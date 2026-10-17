//! Strict Tauri v7 document. The legacy-shaped projection is an IPC/clock adapter,
//! never a reader for Electron documents or files.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
pub const MAX_COUNTER: u64 = 9_007_199_254_740_991;
pub const MAX_BYTES: usize = 1024 * 1024;
pub const CUCKOO: &str = "builtin:cuckoo-clock";
// Retired ID is recognized only to migrate old settings/backups, never played.
pub const REMOVED_WAV: &str = "legacy:wav-cuckoo-clock";
pub const CAPABILITIES: [&str; 2] = ["external-catalog-v1", "audio-routing-v1"];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogRef {
    pub schema_version: u32,
    pub generation: u64,
    pub sha256: String,
}
pub fn sound_id_valid(id: &str) -> bool {
    id == REMOVED_WAV || crate::builtin_audio::find(id).is_some() || crate::sound_library::id_valid(id)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub schema_id: String,
    pub schema_version: u32,
    pub generation: u64,
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub catalog_ref: Option<CatalogRef>,
    pub schedule: Schedule,
    pub appearance: Appearance,
    pub audio: Audio,
    pub presets: Vec<Preset>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub enabled: bool,
    pub slots: Vec<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    pub background_illustration: bool,
    pub minimum_display: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Audio {
    pub volume: u8,
    pub on_hour: OnHour,
    pub on_half_hour: OnHalfHour,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_sounds: Vec<UserSound>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnHour {
    pub sound_id: String,
    pub repeat: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnHalfHour {
    pub sound_id: String,
    #[serde(default = "one_repeat", skip_serializing_if = "is_one_repeat")]
    pub repeat: u8,
}
fn one_repeat() -> u8 { 1 }
fn is_one_repeat(value: &u8) -> bool { *value == 1 }
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub slots: Vec<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserSound {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub sha256: String,
    pub byte_length: u64,
    pub duration_ms: u64,
}

impl Default for Document {
    fn default() -> Self {
        // A source-controlled synthetic example, never a user's configuration.
        {
            let mut d: Self = serde_json::from_str(include_str!(
                "../resources/default-settings.json"
            ))
            .unwrap();
            d.upgrade();
            d
        }
    }
}
fn trim_name(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, '\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}'))
}
fn valid_name(s: &str, limit: usize) -> bool {
    !s.is_empty()
        && s == trim_name(s)
        && s.encode_utf16().count() <= limit
        && !s.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum PresetAction {
    Create { name: String },
    Rename { id: String, name: String },
    Apply { id: String },
    Overwrite { id: String },
    Delete { id: String },
}
fn preset_name(name: &str) -> Result<String, String> {
    let name = trim_name(name);
    if !valid_name(name, 40) {
        return Err("名前は1〜40文字で入力してください。".into());
    }
    Ok(name.into())
}
impl Document {
    /// Pure operation: only presets or schedule.slots can change. Never captures
    /// enabled/volume/repeat/sound/appearance in a v7 preset.
    pub fn edit_preset(&self, action: &Value) -> Result<(Self, String), String> {
        let action: PresetAction =
            serde_json::from_value(action.clone()).map_err(|_| "プリセット操作が不正です。")?;
        let mut next = self.clone();
        let id = match action {
            PresetAction::Create { name } => {
                let name = preset_name(&name)?;
                if next.presets.iter().any(|p| p.name == name) {
                    return Err("同じ名前のプリセットがあります。".into());
                }
                if next.presets.len() >= 100 {
                    return Err("プリセットは100件まで保存できます。".into());
                }
                let id = uuid::Uuid::new_v4().to_string();
                next.presets.push(Preset {
                    id: id.clone(),
                    name,
                    slots: self.schedule.slots.clone(),
                });
                id
            }
            operation => {
                let id = match &operation {
                    PresetAction::Rename { id, .. }
                    | PresetAction::Apply { id }
                    | PresetAction::Overwrite { id }
                    | PresetAction::Delete { id } => id.clone(),
                    _ => unreachable!(),
                };
                let index = next
                    .presets
                    .iter()
                    .position(|p| p.id == id)
                    .ok_or("プリセットが見つかりません。")?;
                match operation {
                    PresetAction::Rename { name, .. } => {
                        let name = preset_name(&name)?;
                        if next
                            .presets
                            .iter()
                            .enumerate()
                            .any(|(n, p)| n != index && p.name == name)
                        {
                            return Err("同じ名前のプリセットがあります。".into());
                        }
                        next.presets[index].name = name;
                    }
                    PresetAction::Apply { .. } => {
                        next.schedule.slots = next.presets[index].slots.clone()
                    }
                    PresetAction::Overwrite { .. } => {
                        next.presets[index].slots = self.schedule.slots.clone()
                    }
                    PresetAction::Delete { .. } => {
                        next.presets.remove(index);
                    }
                    _ => unreachable!(),
                }
                id
            }
        };
        next.validate()?;
        Ok((next, id))
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(r) = &self.catalog_ref {
            if r.schema_version != 1
                || r.generation == 0
                || r.generation > MAX_COUNTER
                || r.sha256.len() != 64
                || !r
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err("InvalidCatalogRef".into());
            }
        }
        if self.schema_id != "cuckoo.tauri.settings" || ![7, 8].contains(&self.schema_version) {
            return Err("UnsupportedSchema".into());
        }
        if !(1..=MAX_COUNTER).contains(&self.generation) {
            return Err("InvalidGeneration".into());
        }
        // T3 deliberately supports no T4 capabilities. Do not erase unknown data.
        if (self.schema_version == 7
            && (!self.required_capabilities.is_empty() || self.catalog_ref.is_some()))
            || (self.schema_version == 8
                && (self.required_capabilities.len() != 2
                    || !CAPABILITIES
                        .iter()
                        .all(|c| self.required_capabilities.iter().any(|v| v == c))))
        {
            return Err("UnsupportedCapabilities".into());
        }
        if self.schedule.slots.len() != 48
            || self.audio.volume > 100
            || ![1, 2].contains(&self.audio.on_hour.repeat)
            || ![1, 2].contains(&self.audio.on_half_hour.repeat)
            || (self.schema_version == 7 && self.audio.on_half_hour.repeat != 1)
            || !["system", "dark", "light"].contains(&self.appearance.theme.as_str())
        {
            return Err("InvalidValue".into());
        }
        if !self.audio.user_sounds.is_empty()
            || (self.schema_version == 7
                && (self.audio.on_hour.sound_id != CUCKOO
                    || self.audio.on_half_hour.sound_id != CUCKOO))
            || (self.schema_version == 8
                && (!sound_id_valid(&self.audio.on_hour.sound_id)
                    || !sound_id_valid(&self.audio.on_half_hour.sound_id)))
        {
            return Err("UnsupportedOrUnknownSound".into());
        }
        if self.presets.len() > 100 {
            return Err("TooManyPresets".into());
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for p in &self.presets {
            if p.id.is_empty()
                || p.id.len() > 128
                || !p
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !valid_name(&p.name, 40)
                || p.slots.len() != 48
                || !ids.insert(&p.id)
                || !names.insert(&p.name)
            {
                return Err("InvalidPreset".into());
            }
        }
        Ok(())
    }
    pub fn upgrade(&mut self) {
        if self.schema_version == 7 {
            self.schema_version = 8;
            self.required_capabilities = CAPABILITIES.iter().map(|s| s.to_string()).collect();
            self.audio.on_hour.sound_id = CUCKOO.into();
            self.audio.on_half_hour.sound_id = CUCKOO.into();
        }
        if self.audio.on_hour.sound_id == REMOVED_WAV { self.audio.on_hour.sound_id = CUCKOO.into(); }
        if self.audio.on_half_hour.sound_id == REMOVED_WAV { self.audio.on_half_hour.sound_id = CUCKOO.into(); }
    }
    pub fn select_sound(&self, target: &str, id: &str) -> Result<Self, String> {
        if self.schema_version != 8 || id == REMOVED_WAV || !sound_id_valid(id) {
            return Err("InvalidSoundId".into());
        }
        let mut d = self.clone();
        match target {
            "hour" => d.audio.on_hour.sound_id = id.into(),
            "halfHour" => d.audio.on_half_hour.sound_id = id.into(),
            _ => return Err("InvalidSoundTarget".into()),
        };
        d.validate()?;
        Ok(d)
    }
    pub fn projection(&self) -> Value {
        json!({"version":6,"enabled":self.schedule.enabled,"slots":self.schedule.slots,"presets":self.presets,
            "audioRouting":{"onHour":self.audio.on_hour,"onHalfHour":self.audio.on_half_hour},"soundId":"otologic-cuckoo-clock","hourlyRepeat":self.audio.on_hour.repeat,"halfHourlyRepeat":self.audio.on_half_hour.repeat,"volume":self.audio.volume,
            "theme":self.appearance.theme,"backgroundIllustration":self.appearance.background_illustration,"minimumDisplay":self.appearance.minimum_display})
    }
    pub fn patched(&self, patch: &Value) -> Result<Self, String> {
        let object = patch.as_object().ok_or("InvalidPatch")?;
        let mut v = self.clone();
        for (k, x) in object {
            match k.as_str() {
                "enabled" => v.schedule.enabled = x.as_bool().ok_or("InvalidEnabled")?,
                "slots" => {
                    v.schedule.slots =
                        serde_json::from_value(x.clone()).map_err(|_| "InvalidSlots")?
                }
                "volume" => {
                    v.audio.volume = x
                        .as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or("InvalidVolume")?
                }
                "hourlyRepeat" => {
                    v.audio.on_hour.repeat = x
                        .as_u64()
                        .and_then(|n| u8::try_from(n).ok())
                        .ok_or("InvalidRepeat")?
                }
                "halfHourlyRepeat" => {
                    v.audio.on_half_hour.repeat = x.as_u64()
                        .and_then(|n| u8::try_from(n).ok()).ok_or("InvalidRepeat")?;
                }
                "theme" => v.appearance.theme = x.as_str().ok_or("InvalidTheme")?.into(),
                "backgroundIllustration" => {
                    v.appearance.background_illustration = x.as_bool().ok_or("InvalidBackground")?
                }
                "minimumDisplay" => {
                    v.appearance.minimum_display = x.as_bool().ok_or("InvalidDisplay")?
                }
                _ => return Err(format!("UnknownPatchKey:{k}")),
            }
        }
        v.validate()?;
        Ok(v)
    }
}

// serde_json::Value normally accepts duplicate object keys. This visitor rejects
// duplicates at every level and caps nesting before constructing the typed model.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "JSON without duplicate keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Unique, E> {
                // JSON Schema integer includes 37.0 / 3.7e1. Canonicalize only
                // exact safe integers; fractional/out-of-range values stay invalid.
                Ok(Unique(
                    if v.is_finite() && v.fract() == 0.0 && v >= 0.0 && v <= MAX_COUNTER as f64 {
                        json!(v as u64)
                    } else {
                        json!(v)
                    },
                ))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(json!(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element::<Unique>()? {
                    v.push(x.0)
                }
                Ok(Unique(Value::Array(v)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut v = serde_json::Map::new();
                while let Some((k, x)) = a.next_entry::<String, Unique>()? {
                    if v.insert(k, x.0).is_some() {
                        return Err(serde::de::Error::custom("DuplicateKey"));
                    }
                }
                Ok(Unique(Value::Object(v)))
            }
        }
        d.deserialize_any(Visitor)
    }
}
pub fn decode(bytes: &[u8]) -> Result<Document, String> {
    if bytes.len() > MAX_BYTES {
        return Err("TooLarge".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    // Check depth outside strings before serde's own recursion limit.
    let (mut depth, mut string, mut escape) = (0u32, false, false);
    for &b in bytes {
        if string {
            if escape {
                escape = false
            } else if b == b'\\' {
                escape = true
            } else if b == b'"' {
                string = false
            }
        } else {
            match b {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 16 {
                        return Err("TooDeep".into());
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => (),
            }
        }
    }
    let unique: Unique = serde_json::from_slice(bytes).map_err(|_| "InvalidJsonOrDuplicateKey")?;
    let v = unique.0;
    if v["schemaId"] != "cuckoo.tauri.settings"
        || !matches!(v["schemaVersion"].as_u64(), Some(7 | 8))
    {
        return Err("UnsupportedSchema".into());
    }
    if v["schemaVersion"] == 7
        && (v.get("catalogRef").is_some() || v["audio"].get("userSounds").is_none())
    {
        return Err("InvalidLegacyShape".into());
    }
    if v["schemaVersion"] == 8
        && (v["audio"].get("userSounds").is_some() || v.get("catalogRef").is_none())
    {
        return Err("InvalidV8Shape".into());
    }
    let doc: Document = serde_json::from_value(v).map_err(|_| "InvalidShapeOrUnknownField")?;
    doc.validate()?;
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_hour_repeat_defaults_roundtrips_and_presets_preserve_both_counts() {
        let d=Document::default();
        assert_eq!(d.audio.on_hour.repeat,2);
        assert_eq!(d.audio.on_half_hour.repeat,1);
        let old=serde_json::to_value(&d).unwrap();
        assert!(old["audio"]["onHalfHour"].get("repeat").is_none());
        let changed=d.patched(&json!({"hourlyRepeat":1,"halfHourlyRepeat":2})).unwrap();
        let restored=decode(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(restored.audio.on_hour.repeat,1);
        assert_eq!(restored.audio.on_half_hour.repeat,2);
        let (saved,id)=restored.edit_preset(&json!({"type":"create","name":"counts"})).unwrap();
        let (applied,_)=saved.edit_preset(&json!({"type":"apply","id":id})).unwrap();
        assert_eq!(applied.audio,restored.audio);
        for value in [json!(0),json!(3),json!("2"),json!(null)] {
            assert!(d.patched(&json!({"halfHourlyRepeat":value})).is_err());
        }
    }
    #[test]
    fn preset_operations_match_slots_only_contract() {
        let original = Document::default();
        let (mut first, id) = original
            .edit_preset(&json!({"type":"create","name":"  Morning  "}))
            .unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(first.presets[0].name, "Morning");
        assert_eq!(original.presets.len(), 0);
        let (second, other) = first
            .edit_preset(&json!({"type":"create","name":"Evening"}))
            .unwrap();
        first = second;
        first.schedule.enabled = false;
        first.schedule.slots.fill(false);
        first.audio.volume = 12;
        first.audio.on_hour.repeat = 1;
        first.appearance.theme = "dark".into();
        let (applied, applied_id) = first.edit_preset(&json!({"type":"apply","id":id})).unwrap();
        let mut expected = first.clone();
        expected.schedule.slots = original.schedule.slots.clone();
        assert_eq!(applied, expected);
        assert_eq!(applied_id, id);
        let (renamed, _) = applied
            .edit_preset(&json!({"type":"rename","id":id,"name":"  Noon  "}))
            .unwrap();
        assert_eq!(renamed.presets[0].id, id);
        assert_eq!(renamed.presets[0].name, "Noon");
        assert_eq!(renamed.presets[1].id, other);
        let mut edited = renamed.clone();
        edited.schedule.slots.fill(true);
        let (overwritten, _) = edited
            .edit_preset(&json!({"type":"overwrite","id":id}))
            .unwrap();
        assert_eq!(overwritten.presets[0].slots, vec![true; 48]);
        assert_eq!(overwritten.presets[1], renamed.presets[1]);
        let (deleted, deleted_id) = overwritten
            .edit_preset(&json!({"type":"delete","id":id}))
            .unwrap();
        assert_eq!(deleted_id, id);
        assert_eq!(deleted.presets, vec![renamed.presets[1].clone()]);
        assert_eq!(deleted.audio, overwritten.audio);
    }
    #[test]
    fn preset_names_limits_and_invalid_actions() {
        let d = Document::default();
        for name in [
            "".into(),
            "   ".into(),
            "a".repeat(41),
            "😀".repeat(21),
            "a\nb".into(),
            "a\u{7f}b".into(),
        ] {
            assert!(d
                .edit_preset(&json!({"type":"create","name":name}))
                .is_err());
        }
        for name in [
            "a".repeat(40),
            "😀".repeat(20),
            "\u{feff}Name\u{feff}".into(),
            "名前".into(),
        ] {
            assert!(d.edit_preset(&json!({"type":"create","name":name})).is_ok());
        }
        let (mut d, id) = d
            .edit_preset(&json!({"type":"create","name":"Name"}))
            .unwrap();
        assert!(d
            .edit_preset(&json!({"type":"create","name":" Name "}))
            .is_err());
        assert!(d
            .edit_preset(&json!({"type":"create","name":"name"}))
            .is_ok());
        assert!(d
            .edit_preset(&json!({"type":"rename","id":id,"name":"Name"}))
            .is_ok());
        for action in [
            Value::Null,
            json!({}),
            json!({"type":"unknown"}),
            json!({"type":"create","name":5}),
            json!({"type":"apply","id":"missing"}),
            json!({"type":"delete","id":null}),
        ] {
            assert!(d.edit_preset(&action).is_err());
        }
        for n in 1..100 {
            d = d
                .edit_preset(&json!({"type":"create","name":format!("preset{n}")}))
                .unwrap()
                .0;
        }
        assert_eq!(d.presets.len(), 100);
        assert!(d
            .edit_preset(&json!({"type":"create","name":"one too many"}))
            .is_err());
        assert!(d
            .edit_preset(&json!({"type":"rename","id":id,"name":"preset1"}))
            .is_err());
    }
    #[test]
    fn fixtures_and_semantics() {
        decode(include_bytes!(
            "../resources/default-settings.json"
        ))
        .unwrap();
        decode(include_bytes!(
            "../test-data/migrated-v7.json"
        ))
        .unwrap();
        assert!(decode(include_bytes!(
            "../test-data/unsupported-v7-capabilities.json"
        ))
        .is_err());
        for patch in [
            json!({"volume":101}),
            json!({"volume":-1}),
            json!({"volume":0.5}),
            json!({"slots":[true]}),
            json!({"enabled":"true"}),
            json!({"settingsPath":"forbidden"}),
        ] {
            assert!(Document::default().patched(&patch).is_err())
        }
        let initial = serde_json::to_value(Document::default()).unwrap();
        for key in initial.as_object().unwrap().keys() {
            let mut v = initial.clone();
            v.as_object_mut().unwrap().remove(key);
            assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err(), "{key}");
        }
        for id in [
            "builtin:unknown",
            "user:11111111-1111-1111-1111-111111111111",
        ] {
            let mut d = Document::default();
            d.audio.on_hour.sound_id = id.into();
            assert!(d.validate().is_err());
        }
        let mut d = Document::default();
        d.presets = vec![Preset {
            id: "a".into(),
            name: "😀".repeat(21),
            slots: vec![false; 48],
        }];
        assert!(d.validate().is_err());
        d.presets[0].name = "valid".into();
        d.presets.push(d.presets[0].clone());
        assert!(d.validate().is_err());
    }
    #[test]
    fn duplicate_depth_and_unknown_fields() {
        let s = serde_json::to_string(&Document::default()).unwrap();
        assert!(decode(
            s.replacen("\"volume\":50", "\"volume\":50,\"volume\":12", 1)
                .as_bytes()
        )
        .is_err());
        assert!(decode(format!("{}0{}", "[".repeat(17), "]".repeat(17)).as_bytes()).is_err());
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["unexpected"] = json!(true);
        assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    #[test]
    fn schema_integer_lexemes_and_utf16_name_rules() {
        let s = serde_json::to_string(&Document::default()).unwrap();
        let d = decode(s.replacen("\"volume\":50", "\"volume\":5e1", 1).as_bytes()).unwrap();
        assert_eq!(d.audio.volume, 50);
        assert!(valid_name("\u{0085}name", 40));
        assert!(!valid_name("\u{feff}name", 40));
        assert!(!valid_name(" name", 40));
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["audio"]["volume"] = json!(37.5);
        assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}
