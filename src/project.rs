use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=8192).contains(&self.width)
                && (1..=8192).contains(&self.height)
                && u64::from(self.width) * u64::from(self.height) <= 32_000_000,
            "Invalid desktop dimensions"
        );
        ensure!([30, 60].contains(&self.fps), "Invalid frame rate");
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Launch {
    pub name: String,
    pub command: String,
    pub directory: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WindowLayout {
    pub app_id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
