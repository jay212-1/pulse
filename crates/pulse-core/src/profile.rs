//! The profile file (`.pulse`): JSON with a schema version, migrated step by
//! step on load and written atomically (a crash never leaves half a file).
//! Export and import are the same file.

use crate::model::{Profile, SCHEMA_VERSION};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;

pub const EXTENSION: &str = "pulse";

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("not a Pulse profile: {0}")]
    Parse(String),
    #[error("this profile was made by a newer Pulse (schema {0}); please update Pulse")]
    TooNew(u32),
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> FileError + '_ {
    move |source| FileError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Bring an older document up to [`SCHEMA_VERSION`], one step at a time.
pub fn migrate(mut doc: Value) -> Result<Value, FileError> {
    let version = doc
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32;
    if version > SCHEMA_VERSION {
        return Err(FileError::TooNew(version));
    }
    // Steps go here: `if version < 2 { migrate_v1_to_v2(&mut doc) }` and so on.
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("schema_version".into(), SCHEMA_VERSION.into());
    }
    Ok(doc)
}

pub fn parse(text: &str) -> Result<Profile, FileError> {
    let doc: Value = serde_json::from_str(text).map_err(|e| FileError::Parse(e.to_string()))?;
    if !doc.is_object() {
        return Err(FileError::Parse("expected a JSON object".into()));
    }
    let doc = migrate(doc)?;
    let mut p: Profile =
        serde_json::from_value(doc).map_err(|e| FileError::Parse(e.to_string()))?;
    repair_ids(&mut p);
    Ok(p)
}

/// Ids unique and `next_id` above all of them (a hand-edited or merged file may be off).
fn repair_ids(p: &mut Profile) {
    let mut seen = std::collections::HashSet::new();
    let mut next = p.max_id().max(p.next_id.saturating_sub(1)) + 1;
    for d in &mut p.devices {
        if d.id == 0 || !seen.insert(d.id) {
            d.id = next;
            seen.insert(next);
            next += 1;
        }
        for c in &mut d.checks {
            if c.id == 0 || !seen.insert(c.id) {
                c.id = next;
                seen.insert(next);
                next += 1;
            }
        }
    }
    p.next_id = next;
}

pub fn to_string(p: &Profile) -> Result<String, FileError> {
    let mut p = p.clone();
    p.schema_version = SCHEMA_VERSION;
    serde_json::to_string_pretty(&p).map_err(|e| FileError::Parse(e.to_string()))
}

pub fn load(path: &Path) -> Result<Profile, FileError> {
    let text = fs::read_to_string(path).map_err(io_err(path))?;
    parse(&text)
}

pub fn save(p: &Profile, path: &Path) -> Result<(), FileError> {
    let text = to_string(p)?;
    atomic_write(path, text.as_bytes())
}

/// Write to a temporary file next to `path`, sync, then rename over it.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), FileError> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).map_err(io_err(dir))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("profile");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp).map_err(io_err(&tmp))?;
        f.write_all(data).map_err(io_err(&tmp))?;
        f.sync_all().map_err(io_err(&tmp))?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path)(e));
    }
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CheckKind, DeviceKind};

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("Aula.pulse");
        let mut p = Profile {
            name: "Aula".into(),
            ..Profile::default()
        };
        let d = p.add_device(DeviceKind::Console, "FOH desk", "10.0.0.1");
        p.add_check(d, CheckKind::Tcp { port: 10023 });
        save(&p, &path).unwrap_or_else(|e| unreachable!("{e}"));
        let back = load(&path).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(back, p);
        // No temporary file left behind.
        let names: Vec<_> = fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn minimal_and_old_files_load_with_defaults() {
        let p = parse(
            r#"{ "name": "X", "devices": [ { "id": 3, "name": "SB", "address": "10.0.0.2",
            "checks": [ { "id": 3, "kind": { "type": "Ping" } } ] } ] }"#,
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(p.schema_version, SCHEMA_VERSION);
        let d = &p.devices[0];
        assert!(d.enabled);
        assert_eq!(d.hysteresis.fail_after, 3);
        // The duplicate id was repaired, next_id is above everything.
        assert_ne!(d.checks[0].id, d.id);
        assert!(p.next_id > p.max_id());
    }

    #[test]
    fn newer_files_and_garbage_are_refused() {
        assert!(matches!(
            parse(r#"{ "schema_version": 99 }"#),
            Err(FileError::TooNew(99))
        ));
        assert!(matches!(parse("[1,2]"), Err(FileError::Parse(_))));
        assert!(matches!(parse("not json"), Err(FileError::Parse(_))));
    }
}
