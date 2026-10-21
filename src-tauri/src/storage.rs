use crate::model::{AppError, Settings};
use std::{fs, path::Path};

pub fn load_settings(root: &Path) -> Result<Settings, AppError> {
    let path = root.join("settings.json");
    if !path.exists() {
        return Ok(Settings::default());
    }
    let bytes = fs::read(path).map_err(|e| AppError::io("读取设置失败", e))?;
    let settings: Settings = serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(
            "invalid_settings",
            format!("设置文件损坏，已使用默认值；原文件保留：{e}"),
        )
    })?;
    crate::engine::validate_target(settings.target_count)?;
    Ok(settings)
}

pub fn save_settings(root: &Path, settings: &Settings) -> Result<(), AppError> {
    fs::create_dir_all(root).map_err(|e| AppError::io("创建设置目录失败", e))?;
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| AppError::new("invalid_settings", e.to_string()))?;
    atomic_write(&root.join("settings.json"), &bytes)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("invalid_path", "无效的输出路径"))?;
    if !parent.is_dir() {
        return Err(AppError::new("invalid_path", "输出目录不存在"));
    }
    let temporary = parent.join(format!(".more-wechat-{}.tmp", std::process::id()));
    fs::write(&temporary, bytes).map_err(|e| AppError::io("写入临时文件失败", e))?;
    let result = replace(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> Result<(), AppError> {
    fs::rename(from, to).map_err(|e| AppError::io("保存文件失败", e))
}

#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> Result<(), AppError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(AppError::io(
            "保存文件失败",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(())
    }
}

pub fn redact(text: &str, home: Option<&str>) -> String {
    let mut output = text.to_string();
    if let Some(home) = home.filter(|s| !s.is_empty()) {
        output = output
            .replace(home, "<用户目录>")
            .replace(&home.replace('\\', "/"), "<用户目录>");
    }
    let user_path = regex::Regex::new(r#"(?i)(?:[a-z]:[\\/]+Users[\\/]+|/Users/)[^\\/"\r\n]+"#)
        .expect("constant user path pattern");
    user_path.replace_all(&output, "<用户目录>").into_owned()
}

pub fn redact_value(value: &mut serde_json::Value, home: Option<&str>) {
    match value {
        serde_json::Value::String(text) => *text = redact(text, home),
        serde_json::Value::Array(values) => {
            for value in values {
                redact_value(value, home);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redact_value(value, home);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_roundtrip_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        save_settings(dir.path(), &Settings::default()).unwrap();
        let settings = Settings {
            install_path: Some("D:\\微信 工作\\Weixin.exe".into()),
            target_count: 3,
        };
        save_settings(dir.path(), &settings).unwrap();
        let loaded = load_settings(dir.path()).unwrap();
        assert_eq!(loaded.install_path, settings.install_path);
        assert_eq!(loaded.target_count, 3);
    }
    #[test]
    fn preserves_corrupt_settings() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.json"), b"broken").unwrap();
        assert!(load_settings(dir.path()).is_err());
        assert_eq!(
            fs::read(dir.path().join("settings.json")).unwrap(),
            b"broken"
        );
    }
    #[test]
    fn exported_paths_hide_usernames() {
        let text = "C:\\Users\\Alice\\微信 /Users/Bob/Applications D:/custom/Carol/微信";
        let redacted = redact(text, Some("D:/custom/Carol"));
        assert!(!redacted.contains("Alice"));
        assert!(!redacted.contains("Bob"));
        assert!(!redacted.contains("Carol"));
    }
    #[test]
    fn redaction_precedes_json_escaping() {
        let mut value =
            serde_json::json!({ "path": "D:\\USERS\\Alice\\Weixin", "error": ["/Users/Bob/Apps"] });
        redact_value(&mut value, None);
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(!serialized.contains("Alice"));
        assert!(!serialized.contains("Bob"));
        assert!(serde_json::from_str::<serde_json::Value>(&serialized).is_ok());
    }
}
