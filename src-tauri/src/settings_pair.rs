//! Immutable transaction evidence. Never completes a partial transaction by guessing.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pair {
    settings: Option<Vec<u8>>,
    catalog: Option<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    id: String,
    previous: Pair,
    candidate: Pair,
    acknowledged: Vec<Vec<u8>>,
}
fn pair(root: &Path) -> Result<Pair, DiskError> {
    Ok(Pair {
        settings: read(&root.join("settings.json"))?.map(|b| sha(&b)),
        catalog: read(&root.join("sounds/catalog.json"))?.map(|b| sha(&b)),
    })
}
pub(super) fn reconcile(root: &Path) -> Result<(), DiskError> {
    let current = pair(root)?;
    let path = root.join("coordination");
    safe(&path)?;
    if !path.exists() {
        return Ok(());
    }
    let mut records = vec![];
    for entry in fs::read_dir(path)? {
        if records.len() >= 4096 {
            return Err(err("CoordinationHistoryLimit"));
        }
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let id = name
            .strip_suffix(".json")
            .ok_or_else(|| err("UnknownCoordinationFile"))?;
        if Uuid::parse_str(id).is_err() {
            return Err(err("InvalidCoordinationToken"));
        }
        let raw = read(&entry.path())?.ok_or_else(|| err("MissingCoordinationJournal"))?;
        let j: Journal =
            serde_json::from_slice(&raw).map_err(|_| err("InvalidCoordinationJournal"))?;
        if j.schema_version != 1
            || j.id != id
            || [
                &j.previous.settings,
                &j.previous.catalog,
                &j.candidate.settings,
                &j.candidate.catalog,
            ]
            .iter()
            .any(|h| h.as_ref().is_some_and(|h| h.len() != 32))
        {
            return Err(err("InvalidCoordinationJournal"));
        }
        records.push((j, sha(&raw)));
    }
    // Prove a subsequent exact pair using immutable transitions, never generation order.
    for (record, record_hash) in &records {
        if records.iter().any(|(anchor, _)| {
            anchor.acknowledged.contains(record_hash)
                && reaches(&anchor.candidate, &current, &records)
        }) {
            continue;
        }
        let mut reachable = vec![record.previous.clone(), record.candidate.clone()];
        for _ in 0..=records.len() {
            if reachable.contains(&current) {
                break;
            }
            let mut changed = false;
            for (next, _) in &records {
                if reachable.contains(&next.previous) && !reachable.contains(&next.candidate) {
                    reachable.push(next.candidate.clone());
                    changed = true
                }
            }
            if !changed {
                return Err(DiskError {
                    code: "PairRecoveryRequired".into(),
                    uncertain: true,
                });
            }
        }
        if !reachable.contains(&current) {
            return Err(err("PairRecoveryRequired"));
        }
    }
    Ok(())
}
fn reaches(start: &Pair, current: &Pair, records: &[(Journal, Vec<u8>)]) -> bool {
    let mut seen = vec![start.clone()];
    for _ in 0..=records.len() {
        if seen.contains(current) {
            return true;
        }
        let before = seen.len();
        for (j, _) in records {
            if seen.contains(&j.previous) && !seen.contains(&j.candidate) {
                seen.push(j.candidate.clone());
            }
        }
        if seen.len() == before {
            break;
        }
    }
    false
}
pub(super) fn begin(
    root: &Path,
    bytes: &[u8],
    catalog: Option<Vec<u8>>,
) -> Result<Pair, DiskError> {
    reconcile(root)?;
    begin_inner(root, bytes, catalog, vec![])
}
pub(super) fn begin_restore(
    root: &Path,
    bytes: &[u8],
    catalog: Option<Vec<u8>>,
) -> Result<Pair, DiskError> {
    let mut acknowledged = vec![];
    let path = root.join("coordination");
    safe(&path)?;
    if path.exists() {
        for entry in fs::read_dir(&path)? {
            if acknowledged.len() >= 4095 {
                return Err(err("CoordinationHistoryLimit"));
            }
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let id = name
                .strip_suffix(".json")
                .filter(|id| Uuid::parse_str(id).is_ok())
                .ok_or_else(|| err("InvalidCoordinationToken"))?;
            let raw = read(&entry.path())?.ok_or_else(|| err("MissingJournal"))?;
            let j: Journal =
                serde_json::from_slice(&raw).map_err(|_| err("InvalidCoordinationJournal"))?;
            if j.schema_version != 1
                || j.id != id
                || [
                    &j.previous.settings,
                    &j.previous.catalog,
                    &j.candidate.settings,
                    &j.candidate.catalog,
                ]
                .iter()
                .any(|h| h.as_ref().is_some_and(|h| h.len() != 32))
            {
                return Err(err("InvalidCoordinationJournal"));
            }
            acknowledged.push(sha(&raw));
        }
    }
    begin_inner(root, bytes, catalog, acknowledged)
}
fn begin_inner(
    root: &Path,
    bytes: &[u8],
    catalog: Option<Vec<u8>>,
    acknowledged: Vec<Vec<u8>>,
) -> Result<Pair, DiskError> {
    let previous = pair(root)?;
    let candidate = Pair {
        settings: Some(sha(bytes)),
        catalog,
    };
    let id = Uuid::new_v4().to_string();
    let j = Journal {
        schema_version: 1,
        id: id.clone(),
        previous,
        candidate: candidate.clone(),
        acknowledged,
    };
    let path = root.join("coordination");
    let _handle = directory(&path)?;
    new_file(
        &path.join(format!("{id}.json")),
        &serde_json::to_vec(&j).map_err(|_| err("JournalSerialize"))?,
    )?;
    Ok(candidate)
}
pub(super) fn finish(root: &Path, expected: &Pair) -> Result<(), DiskError> {
    if &pair(root)? != expected {
        return Err(DiskError {
            code: "PairCommitUncertain".into(),
            uncertain: true,
        });
    }
    Ok(())
}
