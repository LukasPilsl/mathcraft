//! Registers (or removes) the `mathcraft` server in Claude Desktop's `claude_desktop_config.json`.
//!
//! The merge touches only `mcpServers.mathcraft`; every other key is kept. A `.bak` copy of the
//! existing file is written first, and a file that is not valid JSON (or not an object) is never
//! overwritten.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

/// The key under `mcpServers`.
pub const SERVER_NAME: &str = "mathcraft";

/// Where Claude Desktop keeps its configuration, from the environment of the current OS.
///
/// # Errors
/// When the home or `%APPDATA%` folder is unknown.
pub fn default_config_path() -> Result<PathBuf, String> {
    let env = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
    };
    let file = "claude_desktop_config.json";
    if cfg!(target_os = "windows") {
        let appdata = env("APPDATA").ok_or("%APPDATA% is not set")?;
        Ok(appdata.join("Claude").join(file))
    } else if cfg!(target_os = "macos") {
        let home = env("HOME").ok_or("HOME is not set")?;
        Ok(home.join("Library/Application Support/Claude").join(file))
    } else {
        let base = env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .ok_or("HOME is not set")?;
        Ok(base.join("Claude").join(file))
    }
}

fn read_object(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(format!(
                "{} is valid JSON but not an object; not touching it",
                path.display()
            )),
            Err(e) => Err(format!(
                "{} is not valid JSON ({e}); fix or remove it, nothing was changed",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

fn write_config(path: &Path, map: Map<String, Value>) -> Result<(), String> {
    if path.is_file() {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        std::fs::copy(path, PathBuf::from(bak)).map_err(|e| format!("cannot write backup: {e}"))?;
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
    text.push('\n');
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

/// Adds or replaces the `mathcraft` entry in the config at `path`.
///
/// # Errors
/// On invalid JSON, a non-object file or an I/O failure; the file is then unchanged.
pub fn install(path: &Path, exe: &Path, roots: &[String]) -> Result<(), String> {
    let mut map = read_object(path)?;
    let servers = map
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err("\"mcpServers\" in the config is not an object; nothing was changed".into());
    };
    let mut args = Vec::new();
    for root in roots {
        args.push(json!("--root"));
        args.push(json!(root));
    }
    servers.insert(
        SERVER_NAME.to_string(),
        json!({ "command": exe.to_string_lossy(), "args": args }),
    );
    write_config(path, map)
}

/// Removes the `mathcraft` entry. Returns whether one was there.
///
/// # Errors
/// On invalid JSON, a non-object file or an I/O failure.
pub fn uninstall(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let mut map = read_object(path)?;
    let removed = match map.get_mut("mcpServers") {
        Some(Value::Object(servers)) => servers.remove(SERVER_NAME).is_some(),
        _ => false,
    };
    if removed {
        write_config(path, map)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mathcraft-cd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn creates_missing_file_and_folder() {
        let dir = temp_dir("create");
        let path = dir.join("Claude/claude_desktop_config.json");
        install(&path, Path::new("/x/mathcraft-mcp"), &["/docs".into()]).unwrap();
        let v = read(&path);
        assert_eq!(v["mcpServers"]["mathcraft"]["command"], "/x/mathcraft-mcp");
        assert_eq!(
            v["mcpServers"]["mathcraft"]["args"],
            json!(["--root", "/docs"])
        );
    }

    #[test]
    fn preserves_other_keys_and_backs_up() {
        let dir = temp_dir("merge");
        let path = dir.join("c.json");
        let original = r#"{"theme":"dark","mcpServers":{"other":{"command":"o","args":[1]}}}"#;
        std::fs::write(&path, original).unwrap();
        install(&path, Path::new("m"), &["a".into(), "b".into()]).unwrap();
        let v = read(&path);
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["other"]["command"], "o");
        assert_eq!(
            v["mcpServers"]["mathcraft"]["args"],
            json!(["--root", "a", "--root", "b"])
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("c.json.bak")).unwrap(),
            original
        );
        // Running again replaces, never duplicates.
        install(&path, Path::new("m2"), &[]).unwrap();
        assert_eq!(read(&path)["mcpServers"]["mathcraft"]["command"], "m2");
    }

    #[test]
    fn refuses_invalid_json_and_non_objects() {
        let dir = temp_dir("bad");
        let path = dir.join("c.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install(&path, Path::new("m"), &[]).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        assert!(uninstall(&path).is_err());
        std::fs::write(&path, "[1]").unwrap();
        assert!(install(&path, Path::new("m"), &[]).is_err());
        std::fs::write(&path, r#"{"mcpServers":3}"#).unwrap();
        assert!(install(&path, Path::new("m"), &[]).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"mcpServers":3}"#
        );
    }

    #[test]
    fn uninstall_removes_only_ours() {
        let dir = temp_dir("un");
        let path = dir.join("c.json");
        std::fs::write(&path, r#"{"k":1,"mcpServers":{"other":{},"mathcraft":{}}}"#).unwrap();
        assert!(uninstall(&path).unwrap());
        let v = read(&path);
        assert_eq!(v["k"], 1);
        assert!(v["mcpServers"].get("mathcraft").is_none());
        assert!(v["mcpServers"].get("other").is_some());
        assert!(!uninstall(&path).unwrap());
        assert!(!uninstall(&dir.join("missing.json")).unwrap());
    }
}
