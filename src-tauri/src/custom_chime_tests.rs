use crate::{settings_model::{Document, decode}, scheduler::{self, Scheduler}};
use serde_json::{json, Value};
use chrono::{Local, TimeZone};
use std::time::{Duration, Instant};
fn chime(minute:u32)->Value { json!({"id":"11111111-1111-4111-8111-111111111111","minute":minute,"enabled":true,"soundId":"builtin:cuckoo-clock","repeat":2,"volume":80}) }
fn time(day:u32,h:u32,m:u32,s:u32)->chrono::DateTime<Local>{ Local.with_ymd_and_hms(2026,10,day,h,m,s).single().unwrap() }
#[test]
fn legacy_migration_preserves_master_and_defaults_individual_volumes() {
 let mut old=serde_json::to_value(Document::default()).unwrap();
 old["audio"]["volume"]=json!(37);
 let d=decode(&serde_json::to_vec(&old).unwrap()).unwrap();
 assert_eq!(d.audio.volume,37);assert_eq!(d.audio.on_hour.volume,100);assert_eq!(d.audio.on_half_hour.volume,100);assert!(d.schedule.custom.is_empty());
 let old7=include_bytes!("../resources/default-settings.json");let mut d=decode(old7).unwrap();d.upgrade();assert!(d.schedule.custom.is_empty());assert_eq!(d.audio.on_hour.volume,100);
}
#[test]
fn custom_validation_and_roundtrip() {
 let d=Document::default();
 let changed=d.patched(&json!({"customChimes":[chime(1439)],"hourVolume":80,"halfHourVolume":20})).unwrap();
 assert_eq!(decode(&serde_json::to_vec(&changed).unwrap()).unwrap(),changed);
 assert!(d.patched(&json!({"customChimes":[chime(1440)]})).is_err());
 let mut second=chime(3);second["id"]=json!("22222222-2222-4222-8222-222222222222");
 assert!(d.patched(&json!({"customChimes":[chime(3),second]})).is_err());
 for value in [json!(101),json!(-1),json!(50.5)]{assert!(d.patched(&json!({"hourVolume":value})).is_err());}
 let mut bad=chime(3);bad["volume"]=json!(101);assert!(d.patched(&json!({"customChimes":[bad]})).is_err());
 let changed=changed.patched(&json!({"customChimes":[]})).unwrap();assert!(changed.schedule.custom.is_empty());
}
#[test]
fn master_gain_is_composed_once_and_simultaneous_events_keep_both_routes() {
 for (master,individual,expected) in [(100,80,0.8),(50,80,0.4),(0,100,0.0)] {assert!((scheduler::effective_volume(&json!(master),&json!(individual))-expected).abs()<0.00001);}
 let s=Document::default().patched(&json!({"slots":vec![true;48],"volume":50,"hourVolume":60,"halfHourVolume":20,"customChimes":[chime(540)]})).unwrap().projection();
 let events=scheduler::due(540,&s);assert_eq!(events.len(),2);assert_eq!(events[0].key,"regular");assert_ne!(events[0].key,events[1].key);assert_eq!(events[0].volume,0.3);assert_eq!(events[1].volume,0.4);
 assert_eq!(scheduler::due(570,&s)[0].volume,0.1);
}
#[test]
fn minute_boundaries_midnight_sleep_clock_changes_and_restarts() {
 let s=Document::default().patched(&json!({"slots":vec![false;48],"customChimes":[chime(0)]})).unwrap().projection();
 assert_eq!(scheduler::next(time(16,23,59,59),&s),Some(time(17,0,0,0).timestamp_millis()));
 let mut c=Scheduler::default();let mono=Instant::now();
 assert!(!c.poll(time(16,23,59,59),mono,&s).0);assert!(c.poll(time(17,0,0,0),mono+Duration::from_secs(1),&s).0);
 assert!(!c.poll(time(17,0,0,0),mono+Duration::from_millis(1250),&s).0);
 c.poll(time(16,23,59,59),mono+Duration::from_secs(2),&s);assert!(!c.poll(time(17,0,0,0),mono+Duration::from_secs(3),&s).0);
 let mut sleep=Scheduler::default();sleep.poll(time(16,23,59,59),mono,&s);assert!(!sleep.poll(time(17,0,0,1),mono+Duration::from_secs(10),&s).0);
 assert!(!Scheduler::default().poll(time(17,0,0,0),mono,&s).0);
}
#[test]
fn edits_and_disabling_immediately_change_next_event() {
 let s=Document::default().patched(&json!({"slots":vec![false;48],"customChimes":[chime(541)]})).unwrap();
 assert_eq!(scheduler::next(time(16,9,0,0),&s.projection()),Some(time(16,9,1,0).timestamp_millis()));
 let mut disabled=chime(541);disabled["enabled"]=json!(false);
 let d=s.patched(&json!({"customChimes":[disabled]})).unwrap();assert_eq!(scheduler::next(time(16,9,0,0),&d.projection()),None);
}
#[test]
fn custom_changes_are_durable_and_survive_restart_edit_disable_and_delete() {
 use std::sync::Arc;
 use crate::{settings::MemorySettings,settings_disk::Store};
 let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test-runtime/settings").join(uuid::Uuid::new_v4().to_string());
 let boot=|| {let s=Arc::new(MemorySettings::default());let path=root.clone();let worker=s.start_with(move||Store::test(path),||{}).unwrap();s.flush(Duration::from_secs(5)).unwrap();(s,worker)};
 let (s,worker)=boot();let session=s.status()["settingsSync"]["sessionId"].as_str().unwrap().to_string();
 s.apply(&session,0,"add",&json!({"customChimes":[chime(541)]})).unwrap();
 assert_eq!(s.wait_preset("add",Duration::from_secs(5))["state"],"saved");
 s.stop_worker();worker.join().unwrap();
 let(s,worker)=boot();assert_eq!(s.snapshot()["customChimes"][0]["minute"],541);
 let session=s.status()["settingsSync"]["sessionId"].as_str().unwrap().to_string();
 let mut edited=chime(1439);edited["enabled"]=json!(false);edited["volume"]=json!(25);
 s.apply(&session,0,"edit",&json!({"customChimes":[edited]})).unwrap();assert_eq!(s.wait_preset("edit",Duration::from_secs(5))["state"],"saved");
 assert_eq!(s.snapshot()["customChimes"][0]["volume"],25);
 s.apply(&session,1,"delete",&json!({"customChimes":[]})).unwrap();assert_eq!(s.wait_preset("delete",Duration::from_secs(5))["state"],"saved");
 s.stop_worker();worker.join().unwrap();let(s,worker)=boot();assert!(s.snapshot()["customChimes"].as_array().unwrap().is_empty());
 s.stop_worker();worker.join().unwrap();std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn custom_chime_keeps_registered_audio_selected_and_known_failure_does_not_apply() {
 use std::sync::Arc;
 use crate::{settings::MemorySettings,settings_disk::Store,sound_library};
 let(library,sound,blob)=sound_library::Service::test_registered(crate::builtin_audio::ENTRIES[0].bytes);
 let root=blob.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
 let s=Arc::new(MemorySettings::default());let path=root.clone();let worker=s.start_with(move||Store::test(path),||{}).unwrap();s.flush(Duration::from_secs(5)).unwrap();library.attach_settings(&s);
 let session=s.status()["settingsSync"]["sessionId"].as_str().unwrap().to_string();let mut c=chime(541);c["soundId"]=json!(sound.id);
 s.apply(&session,0,"select-custom",&json!({"customChimes":[c]})).unwrap();assert_eq!(s.wait_preset("select-custom",Duration::from_secs(5))["state"],"saved");
 let id=sound.id.clone();let error=library.execute_at(Some((&session,1)),move|store| {let generation=store.snapshot()?.generation;store.unregister(&id,generation)}).unwrap().recv().unwrap().unwrap_err();assert_eq!(error.code,"SoundSelected");
 let before=s.snapshot();let mut bad=chime(542);bad["soundId"]=json!(format!("user:{}",uuid::Uuid::new_v4()));
 // Unknown/invalid source IDs must never silently become a different sound.
 if s.apply(&session,1,"bad",&json!({"customChimes":[bad]})).is_ok() {assert_eq!(s.wait_preset("bad",Duration::from_secs(5))["state"],"failed");}
 assert_eq!(s.snapshot(),before);
 s.stop_worker();worker.join().unwrap();drop(library);std::fs::remove_dir_all(root).unwrap();
}
