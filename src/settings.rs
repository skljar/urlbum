use std::path::Path;

#[derive(Clone)]
pub struct Settings {
    pub show_toolbar:      bool,
    pub collapse_siblings: bool,
    pub confirm_delete:    bool,
    pub no_duplicate_urls: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_toolbar:      true,
            collapse_siblings: false,
            confirm_delete:    true,
            no_duplicate_urls: false,
        }
    }
}

pub fn load_settings(exe_dir: &Path) -> Settings {
    let Ok(content) = std::fs::read_to_string(exe_dir.join("settings.json")) else {
        return Settings::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Settings::default();
    };
    Settings {
        show_toolbar:      v.get("show_toolbar").and_then(|x| x.as_bool()).unwrap_or(true),
        collapse_siblings: v.get("collapse_siblings").and_then(|x| x.as_bool()).unwrap_or(false),
        confirm_delete:    v.get("confirm_delete").and_then(|x| x.as_bool()).unwrap_or(true),
        no_duplicate_urls: v.get("no_duplicate_urls").and_then(|x| x.as_bool()).unwrap_or(false),
    }
}

pub fn save_settings(exe_dir: &Path, s: &Settings) {
    let v = serde_json::json!({
        "show_toolbar":      s.show_toolbar,
        "collapse_siblings": s.collapse_siblings,
        "confirm_delete":    s.confirm_delete,
        "no_duplicate_urls": s.no_duplicate_urls,
    });
    if let Ok(json) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(exe_dir.join("settings.json"), json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn defaults_when_no_file() {
        let s = load_settings(&PathBuf::from("nonexistent_dir_xyz"));
        assert!(s.show_toolbar);
        assert!(!s.collapse_siblings);
        assert!(s.confirm_delete);
        assert!(!s.no_duplicate_urls);
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = std::env::temp_dir().join("urlbum_settings_roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let orig = Settings {
            show_toolbar:      false,
            collapse_siblings: true,
            confirm_delete:    false,
            no_duplicate_urls: true,
        };
        save_settings(&dir, &orig);
        let loaded = load_settings(&dir);
        assert_eq!(loaded.show_toolbar,      orig.show_toolbar);
        assert_eq!(loaded.collapse_siblings, orig.collapse_siblings);
        assert_eq!(loaded.confirm_delete,    orig.confirm_delete);
        assert_eq!(loaded.no_duplicate_urls, orig.no_duplicate_urls);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_keys() {
        let dir = std::env::temp_dir().join("urlbum_settings_partial");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"show_toolbar": false}"#).unwrap();
        let s = load_settings(&dir);
        assert!(!s.show_toolbar);
        assert!(!s.collapse_siblings);
        assert!(s.confirm_delete);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
