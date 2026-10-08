//! Shared Sofia settings storage; the desktop settings UI will build on this crate.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub fn settings_path() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or("XDG_CONFIG_HOME and HOME are unset")?;
        Ok(base.join("sofia/setting.json"))
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA").ok_or("APPDATA is unset")?;
        Ok(PathBuf::from(base).join("Sofia/setting.json"))
    }
}

pub fn load_output_device_id() -> Result<Option<String>, String> {
    load_output_device_id_at(&settings_path()?)
}

pub fn save_output_device_id(id: Option<&str>) -> Result<(), String> {
    save_output_device_id_at(&settings_path()?, id)
}

pub fn load_gemini_voice_name() -> Result<Option<String>, String> {
    load_string_at(&settings_path()?, "gemini", "voice_name")
}

pub fn save_gemini_voice_name(name: Option<&str>) -> Result<(), String> {
    save_string_at(&settings_path()?, "gemini", "voice_name", name)
}

fn load_output_device_id_at(path: &Path) -> Result<Option<String>, String> {
    load_string_at(path, "audio", "output_device_id")
}

fn load_string_at(path: &Path, section: &str, key: &str) -> Result<Option<String>, String> {
    let Some(root) = read_root(path)? else {
        return Ok(None);
    };
    Ok(root
        .get(section)
        .and_then(|section| section.get(key))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn save_output_device_id_at(path: &Path, id: Option<&str>) -> Result<(), String> {
    save_string_at(path, "audio", "output_device_id", id)
}

fn save_string_at(
    path: &Path,
    section: &str,
    key: &str,
    value: Option<&str>,
) -> Result<(), String> {
    let mut root = read_root(path)?.unwrap_or_else(|| Value::Object(Map::new()));
    let object = root
        .as_object_mut()
        .ok_or("settings root must be a JSON object")?;
    let section_object = object
        .entry(section)
        .or_insert_with(|| Value::Object(Map::new()));
    let section_object = section_object
        .as_object_mut()
        .ok_or("settings section must be a JSON object")?;
    if let Some(value) = value {
        section_object.insert(key.into(), Value::String(value.into()));
    } else {
        section_object.remove(key);
    }
    let directory = path
        .parent()
        .ok_or("settings path needs a parent directory")?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temp = path.with_extension(format!("json.tmp.{}.{}", std::process::id(), stamp));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|error| error.to_string())?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, &root).map_err(|error| error.to_string())?;
        file.write_all(b"\n").map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temp, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn read_root(path: &Path) -> Result<Option<Value>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_selection_preserves_other_settings() {
        let dir = std::env::temp_dir().join(format!("sofia-settings-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("setting.json");
        fs::write(
            &path,
            r#"{"theme":"dark","audio":{"input_device_id":"mic"}}"#,
        )
        .unwrap();
        save_output_device_id_at(&path, Some("device-one")).unwrap();
        assert_eq!(
            load_output_device_id_at(&path).unwrap().as_deref(),
            Some("device-one")
        );
        let root = read_root(&path).unwrap().unwrap();
        assert_eq!(root["theme"], "dark");
        assert_eq!(root["audio"]["input_device_id"], "mic");
        save_string_at(&path, "gemini", "voice_name", Some("Kore")).unwrap();
        assert_eq!(
            load_string_at(&path, "gemini", "voice_name")
                .unwrap()
                .as_deref(),
            Some("Kore")
        );
        assert_eq!(
            load_output_device_id_at(&path).unwrap().as_deref(),
            Some("device-one")
        );
        save_output_device_id_at(&path, None).unwrap();
        assert_eq!(load_output_device_id_at(&path).unwrap(), None);
        fs::remove_dir_all(dir).unwrap();
    }
}
