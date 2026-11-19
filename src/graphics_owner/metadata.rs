//! Retained source metadata. Clipboard reads are never serviced or forwarded.
use crate::graphics_resources::{Budget, Lease};
use anyhow::{Result, ensure};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
const TITLE: usize = 4096;
const CLIPBOARD: usize = 65536;
const RESERVATION: usize = 192 * 1024;
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub title: String,
    pub icon_name: String,
    pub audible: u64,
    pub visual: u64,
    pub clipboard_sequence: u64,
    pub clipboard: Option<String>,
    pub rejected: u64,
}
impl Metadata {
    pub fn projection(&self, after: u64) -> Self {
        Self {
            title: self.title.clone(),
            icon_name: self.icon_name.clone(),
            audible: self.audible,
            visual: self.visual,
            clipboard_sequence: self.clipboard_sequence,
            clipboard: (self.clipboard_sequence > after)
                .then(|| self.clipboard.clone())
                .flatten(),
            rejected: self.rejected,
        }
    }
    pub fn clipboard_after<'a>(&'a self, seen: &mut u64, floor: u64) -> Option<&'a str> {
        *seen = (*seen).max(floor);
        if self.clipboard_sequence <= *seen {
            return None;
        }
        *seen = self.clipboard_sequence;
        self.clipboard.as_deref()
    }
    pub fn preflight(value: &Value) -> Result<usize> {
        if value.is_null() {
            return Ok(0);
        }
        let object = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Invalid terminal metadata"))?;
        ensure!(object.len() == 7, "Invalid terminal metadata fields");
        let mut bytes = 0;
        for field in ["title", "icon_name"] {
            let text = value[field]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid terminal title"))?;
            ensure!(
                text.len() <= TITLE && !text.chars().any(char::is_control),
                "Invalid terminal title"
            );
            bytes += text.len();
        }
        for field in ["audible", "visual", "clipboard_sequence", "rejected"] {
            ensure!(
                value[field].as_u64().is_some(),
                "Invalid terminal metadata sequence"
            );
        }
        if !value["clipboard"].is_null() {
            let text = value["clipboard"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid terminal clipboard text"))?;
            ensure!(
                text.len() <= CLIPBOARD && value["clipboard_sequence"].as_u64().unwrap() > 0,
                "Invalid terminal clipboard event"
            );
            bytes += text.len();
        }
        ensure!(
            object.contains_key("clipboard"),
            "Missing terminal clipboard field"
        );
        Ok(bytes * 4 + 4096)
    }
    pub fn memory_bytes(&self) -> usize {
        4 * (self.title.capacity()
            + self.icon_name.capacity()
            + self.clipboard.as_ref().map_or(0, String::capacity))
            + 4096
    }
}
pub struct Capture {
    pub value: Metadata,
    budget: Budget,
    memory: Option<Lease>,
}
impl Capture {
    pub fn new(budget: Budget) -> Self {
        Self {
            value: Metadata::default(),
            budget,
            memory: None,
        }
    }
    fn admit(&mut self) -> bool {
        if self.memory.is_some() {
            return true;
        }
        self.memory = self.budget.reserve(RESERVATION);
        if self.memory.is_none() {
            self.reject();
            return false;
        }
        true
    }
    fn reject(&mut self) {
        self.value.rejected = self.value.rejected.saturating_add(1);
    }
    pub fn title(&mut self, bytes: &[u8], icon: bool) {
        let Ok(text) = std::str::from_utf8(bytes) else {
            self.reject();
            return;
        };
        if text.len() > TITLE || text.chars().any(char::is_control) {
            self.reject();
            return;
        }
        if !self.admit() {
            return;
        }
        if icon {
            self.value.icon_name = text.into();
        } else {
            self.value.title = text.into();
        }
    }
    pub fn copy(&mut self, data: &[u8]) {
        if data.len() > 87384 {
            self.reject();
            return;
        }
        // Reserve the retained value and transient decode before allocating.
        if !self.admit() {
            return;
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            self.reject();
            return;
        };
        if bytes.len() > CLIPBOARD {
            self.reject();
            return;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            self.reject();
            return;
        };
        let Some(sequence) = self.value.clipboard_sequence.checked_add(1) else {
            self.reject();
            return;
        };
        self.value.clipboard = Some(text);
        self.value.clipboard_sequence = sequence;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_cursor_skips_old_attach_hidden_and_reconnect_values() {
        let mut event = Metadata {
            clipboard_sequence: 1,
            clipboard: Some("first".into()),
            ..Default::default()
        };
        let mut seen = 0;
        assert!(event.clipboard_after(&mut seen, 1).is_none());
        event.clipboard_sequence = 2;
        event.clipboard = Some("next".into());
        assert_eq!(event.clipboard_after(&mut seen, 1), Some("next"));
        assert!(event.clipboard_after(&mut seen, 1).is_none());
        event.clipboard_sequence = 3;
        event.clipboard = Some("hidden".into());
        assert!(event.clipboard_after(&mut seen, 3).is_none());
        assert!(event.clipboard_after(&mut seen, 1).is_none());
        event.clipboard_sequence = 4;
        event.clipboard = Some(String::new());
        assert_eq!(event.clipboard_after(&mut seen, 3), Some(""));
        assert!(event.clipboard_after(&mut seen, 4).is_none());
    }
    #[test]
    fn acknowledged_clipboard_text_is_omitted_from_subsequent_frames() {
        let metadata = Metadata {
            title: "retained title".into(),
            clipboard_sequence: 7,
            clipboard: Some("x".repeat(CLIPBOARD)),
            ..Default::default()
        };
        assert_eq!(
            metadata.projection(6).clipboard.as_ref().unwrap().len(),
            CLIPBOARD
        );
        let acknowledged = metadata.projection(7);
        assert_eq!(acknowledged.clipboard_sequence, 7);
        assert_eq!(acknowledged.title, "retained title");
        assert!(acknowledged.clipboard.is_none());
        let wire = serde_json::to_value(&acknowledged).unwrap();
        assert!(Metadata::preflight(&wire).is_ok());
        assert!(serde_json::to_vec(&wire).unwrap().len() < 256);
        assert!(metadata.projection(u64::MAX).clipboard.is_none());
    }
    #[test]
    fn bounded_capture_preserves_previous_values_and_releases_its_lease() {
        let budget = Budget::new(RESERVATION);
        let mut capture = Capture::new(budget.clone());
        capture.title("shell 界".as_bytes(), false);
        capture.title(b"icon", true);
        capture.copy(b"aGVsbG8K");
        assert_eq!(capture.value.title, "shell 界");
        assert_eq!(capture.value.icon_name, "icon");
        assert_eq!(capture.value.clipboard.as_deref(), Some("hello\n"));
        assert_eq!(capture.value.clipboard_sequence, 1);
        capture.title(b"bad\x1btitle", false);
        capture.copy(b"/w==");
        capture.copy(&vec![b'A'; 87385]);
        assert_eq!(capture.value.title, "shell 界");
        assert_eq!(capture.value.clipboard_sequence, 1);
        assert_eq!(capture.value.rejected, 3);
        let value = serde_json::to_value(&capture.value).unwrap();
        assert!(Metadata::preflight(&value).unwrap() >= capture.value.memory_bytes());
        assert_eq!(budget.used(), RESERVATION);
        drop(capture);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn exhausted_budget_refuses_metadata_without_allocating_or_faulting_the_source() {
        let mut capture = Capture::new(Budget::new(1));
        capture.title(b"x", false);
        capture.copy(b"eA==");
        assert!(capture.value.title.is_empty());
        assert_eq!(capture.value.clipboard_sequence, 0);
        assert_eq!(capture.value.rejected, 2);
    }
    #[test]
    fn untrusted_metadata_is_bounded_before_owned_decode() {
        let valid = serde_json::to_value(Metadata::default()).unwrap();
        assert!(Metadata::preflight(&valid).is_ok());
        assert_eq!(Metadata::preflight(&Value::Null).unwrap(), 0);
        for (key, value) in [
            ("title", Value::String("x".repeat(TITLE + 1))),
            ("title", Value::String("bad\x1b".into())),
            ("audible", Value::from(-1)),
            ("clipboard", Value::String("x".into())),
        ] {
            let mut invalid = valid.clone();
            invalid[key] = value;
            assert!(Metadata::preflight(&invalid).is_err());
        }
        let mut invalid = valid;
        invalid["extra"] = Value::Bool(true);
        assert!(Metadata::preflight(&invalid).is_err());
    }
}
