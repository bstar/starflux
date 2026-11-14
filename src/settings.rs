//! Application preferences live independently of project and session metadata.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default = "crate::appearance::default_theme")]
    pub theme: String,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: crate::appearance::default_theme(),
            extra: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn path() -> Result<PathBuf> {
        let directory = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|home| home.join(".config")))
            .context("No configuration directory")?;
        Ok(directory.join("starflux/config.json"))
    }

    fn read(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("Read application settings {}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                theme: crate::appearance::default_theme(),
                extra: BTreeMap::new(),
            }),
            Err(error) => Err(error.into()),
        }
    }

    pub fn appearance() -> Result<crate::appearance::Appearance> {
        let settings = Self::read(&Self::path()?)?;
        crate::appearance::Appearance::load(&settings.theme)
    }

    pub fn set_theme(theme: &str) -> Result<crate::appearance::Appearance> {
        Self::write_theme(&Self::path()?, theme)
    }

    fn write_theme(path: &Path, theme: &str) -> Result<crate::appearance::Appearance> {
        let appearance = crate::appearance::Appearance::load(theme)?;
        let mut settings = Self::read(path)?;
        settings.theme = appearance.id.clone();
        fs::create_dir_all(path.parent().context("No settings parent")?)?;
        crate::automation::persist(path, &serde_json::to_vec_pretty(&settings)?)?;
        Ok(appearance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_and_missing_theme_follow_system() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        assert_eq!(Settings::read(&path).unwrap().theme, "system");
        fs::write(&path, b"{}").unwrap();
        assert_eq!(Settings::read(&path).unwrap().theme, "system");
    }

    #[test]
    fn theme_changes_preserve_other_preferences_and_reject_invalid_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        fs::write(
            &path,
            br#"{"theme":"kanagawa","future_setting":{"enabled":true}}"#,
        )
        .unwrap();
        Settings::write_theme(&path, "system").unwrap();
        let saved = fs::read(&path).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(value["theme"], "system");
        assert_eq!(value["future_setting"]["enabled"], true);
        assert!(Settings::write_theme(&path, "not-a-theme").is_err());
        assert_eq!(fs::read(&path).unwrap(), saved);
    }
}
