//! IDs, never user-supplied URLs, arguments, or executables cross this boundary.
pub fn target(id: &str) -> Result<&'static str, String> {
    match id {
        "voicevoxDownload" => Ok("https://voicevox.hiroshiba.jp/"),
        "otologic" => Ok("https://otologic.jp/"),
        "license" => Ok("https://creativecommons.org/licenses/by/4.0/"),
        "voicevox" => Ok("https://voicevox.hiroshiba.jp/term/"),
        "zundamon" => Ok("https://zunko.jp/con_ongen_kiyaku.html"),
        "himari" => Ok("https://www.meimeihimari.com/terms-of-use"),
        _ => Err("クレジットリンクが不正です。".into()),
    }
}
pub fn resolve(id: &str) -> Result<String, String> {
    if let Some(value)=id.strip_prefix("voicevox-policy:") {
        policy_url(value)
    } else {target(id).map(str::to_owned)}
}
pub fn open(id: &str) -> Result<(), String> {
    let url = resolve(id)?;
    let verb = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let url = url.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    } as isize;
    if result <= 32 {
        Err(format!(
            "既定ブラウザーでリンクを開けませんでした（{result}）。"
        ))
    } else {
        Ok(())
    }
}
fn policy_url(value:&str)->Result<String,String>{
    let url=tauri::Url::parse(value).map_err(|_|"InvalidPolicyUrl")?;
    if !matches!(url.scheme(),"https"|"http")||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||value.chars().any(char::is_control){return Err("InvalidPolicyUrl".into());}
    Ok(url.to_string())
}
pub fn navigation_allowed(url: &tauri::Url) -> bool {
    (url.scheme() == "http"
        && url.host_str() == Some("tauri.localhost")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none())
        || (cfg!(debug_assertions)
            && url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port() == Some(1420)
            && url.username().is_empty()
            && url.password().is_none())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_links_only_open_web_urls(){
        assert!(policy_url("https://example.com/terms").is_ok());
        for value in ["file:///C:/dummy","javascript:alert(1)","https://user:pass@example.com/","cmd.exe","https://example.com/\n"]{assert!(policy_url(value).is_err());}
    }
    #[test]
    fn command_and_browser_share_link_validation(){
        assert_eq!(resolve("voicevox-policy:https://example.com/terms").unwrap(),"https://example.com/terms");
        assert_eq!(resolve("otologic").unwrap(),target("otologic").unwrap());
        for value in ["voicevox-policy:javascript:alert(1)","voicevox-policy:file:///C:/dummy","voicevox-policy:https://user:pass@example.com/","unknown"] {assert!(resolve(value).is_err());}
        let command=include_str!("main.rs").split("async fn open_credit(").nth(1).unwrap().split("enum WindowAction").next().unwrap();
        assert!(command.contains("external_links::resolve(&link)?"));
        assert!(!command.contains("external_links::target(&link)?"));
    }
    #[test]
    fn only_exact_fixed_ids() {
        let links:serde_json::Value=serde_json::from_str(r#"{"otologic":"https://otologic.jp/","license":"https://creativecommons.org/licenses/by/4.0/"}"#).unwrap();
        let source = include_str!("../../src/shared/credits.ts");
        for (id, url) in links.as_object().unwrap() {
            assert_eq!(target(id).unwrap(), url.as_str().unwrap());
            assert!(source.contains(url.as_str().unwrap()));
        }
        for input in [
            "",
            "__proto__",
            "constructor",
            "https://otologic.jp/",
            "file:///C:/dummy",
            "javascript:alert(1)",
            "cmd.exe /c test",
            "otologic\n",
            "OTologic",
            "ms-settings:",
        ] {
            assert!(target(input).is_err());
        }
    }
    #[test]
    fn external_navigation_is_not_an_open_url_api() {
        assert!(navigation_allowed(
            &tauri::Url::parse("http://tauri.localhost/index.html").unwrap()
        ));
        for url in [
            "https://otologic.jp/",
            "http://tauri.localhost.evil/",
            "http://evil@tauri.localhost/",
            "file:///C:/dummy",
            "javascript:alert(1)",
        ] {
            assert!(!navigation_allowed(&tauri::Url::parse(url).unwrap()));
        }
    }
}
