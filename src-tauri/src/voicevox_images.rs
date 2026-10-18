//! Fetch only Engine-provided raster icons. Never expose arbitrary URL fetching to IPC.
use super::*;
const MAX_IMAGE: usize = 512 * 1024;
fn resource_path(source: &str) -> Result<String, String> {
    let url = tauri::Url::parse(&format!("http://127.0.0.1:50021{source}"))
        .ok()
        .filter(|_| source.starts_with('/'))
        .or_else(|| tauri::Url::parse(source).ok())
        .ok_or("InvalidVoicevoxImageUrl")?;
    let name = url
        .path()
        .strip_prefix("/_resources/")
        .ok_or("InvalidVoicevoxImageUrl")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port() != Some(50021)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("InvalidVoicevoxImageUrl".into());
    }
    Ok(url.path().into())
}
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        result.push(TABLE[(a >> 2) as usize] as char);
        result.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
fn raster(bytes: &[u8]) -> Result<String, String> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        return Err("InvalidVoicevoxImage".into());
    };
    if bytes.len() > MAX_IMAGE {
        return Err("VoicevoxImageTooLarge".into());
    }
    Ok(format!("data:image/{mime};base64,{}", base64(bytes)))
}
fn encoded(source: &str) -> Result<String, String> {
    let value = if source.starts_with("data:") {
        source
            .split_once(";base64,")
            .map(|(_, s)| s)
            .ok_or("InvalidVoicevoxImage")?
    } else {
        source
    };
    let mime = if value.starts_with("iVBORw0KGgo") {
        "png"
    } else if value.starts_with("/9j/") {
        "jpeg"
    } else if value.starts_with("R0lGOD") {
        "gif"
    } else if value.starts_with("UklGR") {
        "webp"
    } else {
        return Err("InvalidVoicevoxImage".into());
    };
    let content = value.trim_end_matches('=');
    let padding = value.len() - content.len();
    if value.len() > MAX_IMAGE.div_ceil(3) * 4
        || value.len() % 4 != 0
        || padding > 2
        || !content
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/'))
    {
        return Err("InvalidVoicevoxImage".into());
    }
    Ok(format!("data:image/{mime};base64,{value}"))
}
pub(super) fn thumbnail(
    uuid: &str,
    style_id: u32,
    request: &mut impl FnMut(&str, &str, &[u8], usize) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
    let uuid = uuid::Uuid::parse_str(uuid).map_err(|_| "InvalidSpeakerUuid")?;
    let route = format!("/speaker_info?speaker_uuid={uuid}&resource_format=url");
    let info = json_response(&request("GET", &route, &[], 4 * 1024 * 1024)?)?;
    let icons = info["style_infos"]
        .as_array()
        .ok_or("MissingVoicevoxImage")?;
    let image = icons
        .iter()
        .find(|v| v["id"].as_u64() == Some(style_id as u64))
        .and_then(|v| v["icon"].as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            icons
                .iter()
                .find_map(|v| v["icon"].as_str().filter(|s| !s.is_empty()))
        })
        .or_else(|| info["portrait"].as_str())
        .ok_or("MissingVoicevoxImage")?;
    if image.starts_with("http:")
        || image.starts_with("https:")
        || image.starts_with('/') && !image.starts_with("/9j/")
    {
        let route = resource_path(image)?;
        raster(&request("GET", &route, &[], MAX_IMAGE)?)
    } else {
        encoded(image)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a running external VOICEVOX Engine"]
    fn live_thumbnail() {
        let connection = check().unwrap();
        let speaker = &connection["characters"][0];
        let image = thumbnail(
            speaker["uuid"].as_str().unwrap(),
            speaker["styles"][0]["id"].as_u64().unwrap() as u32,
            &mut crate::voicevox_http::request,
        )
        .unwrap();
        assert!(image.starts_with("data:image/png;base64,"));
        assert!(image.len() > 1000);
    }
    #[test]
    fn resource_urls_are_local_and_exact() {
        assert_eq!(
            resource_path("http://127.0.0.1:50021/_resources/abcd0123").unwrap(),
            "/_resources/abcd0123"
        );
        assert!(resource_path("/_resources/abcd").is_ok());
        for url in [
            "https://example.com/_resources/abcd",
            "http://localhost:50021/_resources/abcd",
            "http://127.0.0.1:50022/_resources/abcd",
            "http://user@127.0.0.1:50021/_resources/abcd",
            "http://127.0.0.1:50021/_resources/abcd?q=1",
            "/_resources/../version",
            "/_resources/%2e%2e",
            "file:///tmp/icon",
            "data:image/svg+xml,test",
        ] {
            assert!(resource_path(url).is_err(), "{url}");
        }
    }
    #[test]
    fn base64_and_raster_limits() {
        for (bytes, want) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
        ] {
            assert_eq!(base64(bytes), want);
        }
        assert!(raster(b"<svg></svg>").is_err());
        assert!(raster(&vec![0; MAX_IMAGE + 1]).is_err());
        let data = raster(b"\x89PNG\r\n\x1a\n\0\0\0\0").unwrap();
        assert_eq!(encoded(&data).unwrap(), data);
        assert!(encoded("iVBORw0KGgo<script>").is_err());
        assert!(encoded("data:image/svg+xml;base64,PHN2Zz4=").is_err());
    }
    #[test]
    fn icon_priority_base64_compatibility_and_failures() {
        let mut calls = vec![];
        let image=thumbnail("11111111-1111-4111-8111-111111111111",8,&mut |_,route,_,limit|{
            calls.push(route.to_string());Ok(if route.starts_with("/speaker_info?"){serde_json::to_vec(&json!({"portrait":"http://evil/image","style_infos":[{"id":7,"icon":"/_resources/aaaa"},{"id":8,"icon":"http://127.0.0.1:50021/_resources/bbbb"}]})).unwrap()}else{assert_eq!(route,"/_resources/bbbb");assert_eq!(limit,MAX_IMAGE);b"\x89PNG\r\n\x1a\n\0\0\0\0".to_vec()})
        }).unwrap();
        assert_eq!(calls.len(), 2);
        assert!(image.starts_with("data:image/png;base64,"));
        let mut base64_reply = |_: &str, _: &str, _: &[u8], _: usize| {
            Ok(serde_json::to_vec(&json!({"style_infos":[{"id":8,"icon":image}]})).unwrap())
        };
        assert_eq!(
            thumbnail("11111111-1111-4111-8111-111111111111", 8, &mut base64_reply).unwrap(),
            image
        );
        assert!(thumbnail("invalid", 8, &mut |_, _, _, _| panic!(
            "invalid UUID must not reach network"
        ))
        .is_err());
        assert!(thumbnail(
            "11111111-1111-4111-8111-111111111111",
            8,
            &mut |_, _, _, _| Err("offline".into())
        )
        .is_err());
    }
}
