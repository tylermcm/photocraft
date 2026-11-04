//! Image Triage metadata replacement, only through the host output service.
//! Ordinary automation writes keep upstream's image/document-only overwrite rule.

use super::{AuthorizedWorkspace, Dir, Path, read_to_observed_length};
use crate::AutomationError;
use serde_json::Value;

const MAX_SIDECAR_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Recipe,
    Manifest,
}

impl AuthorizedWorkspace {
    /// Write a hosted output through the ordinary directory capability and atomic writer.
    /// Existing JSON is replaceable only at the two host sidecar suffixes, when both the
    /// existing and replacement bytes have the matching versioned host schema.
    pub fn write_host_output(&self, path: &str, bytes: &[u8]) -> Result<(), AutomationError> {
        self.write_checked(path, bytes, true)
    }
}

pub(super) fn replacement_allowed(dir: &Dir, path: &Path, bytes: &[u8]) -> bool {
    let Some(kind) = sidecar_kind(path, bytes) else { return false };
    let Ok(metadata) = dir.metadata(path) else { return false };
    if !metadata.is_file() || metadata.len() > MAX_SIDECAR_BYTES {
        return false;
    }
    let Ok(mut file) = dir.open(path) else { return false };
    read_to_observed_length(&mut file, metadata.len()).ok().is_some_and(|previous| sidecar_kind(path, &previous) == Some(kind))
}

fn sidecar_kind(path: &Path, bytes: &[u8]) -> Option<Kind> {
    if bytes.len() as u64 > MAX_SIDECAR_BYTES {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let value: Value = serde_json::from_slice(bytes).ok()?;
    if value.get("version").and_then(Value::as_u64) != Some(1) {
        return None;
    }
    if name.ends_with(".cameraraw.json") && value.get("cameraRaw").is_some_and(Value::is_object) && value.get("identity").is_none_or(Value::is_boolean) {
        return Some(Kind::Recipe);
    }
    if name.ends_with(".photocraft.renders.json")
        && matches!(value.get("kind").and_then(Value::as_str), Some("project" | "recipe"))
        && value.get("generation").and_then(Value::as_u64).is_some()
        && value.get("outputs").is_some_and(Value::is_object)
        && value.get("renderer").and_then(|r| r.get("algorithm")).and_then(Value::as_u64).is_some()
    {
        return Some(Kind::Manifest);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workspace(name: &str) -> (std::path::PathBuf, AuthorizedWorkspace) {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let inside = std::env::temp_dir().join(format!("photocraft-host-sidecar-{}-{name}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&inside).unwrap();
        let workspace = AuthorizedWorkspace::new(Some(&inside), Some(&inside)).unwrap();
        (inside, workspace)
    }

    fn recipe(exposure: i32) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":1,"cameraRaw":{"exposure":exposure},"identity":false})).unwrap()
    }

    fn manifest(generation: u64) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":1,"kind":"recipe","generation":generation,"renderer":{"algorithm":3},"outputs":{}})).unwrap()
    }

    #[test]
    fn host_sidecars_replace_after_restart_but_ordinary_automation_stays_restricted() {
        let (inside, first) = workspace("host-sidecar-restart");
        for (path, before, after) in [("photo.dng.cameraraw.json", recipe(1), recipe(2)), ("photo.dng.photocraft.renders.json", manifest(1), manifest(2))] {
            first.write_host_output(path, &before).unwrap();
            assert!(first.write(path, &after).is_err(), "ordinary automation must not replace JSON");
            let reopened = AuthorizedWorkspace::new(Some(&inside), Some(&inside)).unwrap();
            reopened.write_host_output(path, &after).unwrap();
            assert_eq!(std::fs::read(inside.join(path)).unwrap(), after);
        }
    }

    #[test]
    fn host_sidecars_refuse_unrelated_corrupt_or_mismatched_json_without_changing_it() {
        let (inside, workspace) = workspace("host-sidecar-denials");
        for (path, previous, next) in [
            ("settings.json", recipe(1), recipe(2)),
            ("photo.dng.cameraraw.json", b"private notes".to_vec(), recipe(2)),
            ("photo.dng.photocraft.renders.json", recipe(1), manifest(2)),
            ("other.dng.cameraraw.json", recipe(1), b"invalid JSON".to_vec()),
            ("version.dng.cameraraw.json", recipe(1), br#"{"version":2,"cameraRaw":{}}"#.to_vec()),
        ] {
            workspace.write(path, &previous).unwrap();
            assert!(workspace.write_host_output(path, &next).is_err(), "{path}");
            assert_eq!(std::fs::read(inside.join(path)).unwrap(), previous);
        }
    }

    #[test]
    fn host_sidecars_keep_root_and_regular_file_requirements() {
        let (inside, workspace) = workspace("host-sidecar-roots");
        assert!(AuthorizedWorkspace::default().write_host_output("x.cameraraw.json", &recipe(1)).is_err());
        assert!(workspace.write_host_output("../x.cameraraw.json", &recipe(1)).is_err());
        std::fs::create_dir(inside.join("dir.cameraraw.json")).unwrap();
        assert!(workspace.write_host_output("dir.cameraraw.json", &recipe(1)).is_err());
        workspace.write_host_output("image.png", b"first").unwrap();
        workspace.write_host_output("image.png", b"second").unwrap();
        assert_eq!(std::fs::read(inside.join("image.png")).unwrap(), b"second");
    }
}
