//! Shared shortcuts. Unrecognized prefix sequences are returned to the application.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Chord {
    code: String,
    modifiers: u8,
}
impl Chord {
    fn event(code: &str, modifiers: u8) -> Self {
        let uppercase = code
            .strip_prefix("char:")
            .is_some_and(|s| s.chars().next().is_some_and(|c| c.is_ascii_uppercase()));
        let mut code = code.to_ascii_lowercase();
        if code == "space" {
            code = "char: ".into();
        }
        let mut modifiers = modifiers | u8::from(uppercase);
        if code == "backtab" {
            code = "tab".into();
            modifiers |= 1;
        }
        if let Some(n) = code.strip_prefix("f:") {
            code = format!("f{n}");
        }
        if let Some(c) = code.strip_prefix("char:").and_then(|s| s.chars().next()) {
            let shifted = "~!@#$%^&*()_+{}|:\"<>?";
            let base = "`1234567890-=[]\\;',./";
            if let Some(i) = shifted.chars().position(|s| s == c) {
                code = format!("char:{}", base.chars().nth(i).unwrap());
                modifiers |= 1;
            }
        }
        Self { code, modifiers }
    }
    fn parse(text: &str) -> Result<Self> {
        let parts = text.split('+').collect::<Vec<_>>();
        let mut modifiers = 0;
        for part in &parts[..parts.len() - 1] {
            modifiers |= match part.to_ascii_lowercase().as_str() {
                "ctrl" => 2,
                "alt" => 4,
                "shift" => 1,
                _ => bail!("Unknown modifier in {text:?}"),
            };
        }
        let key = parts.last().unwrap().to_ascii_lowercase();
        let code = if key.chars().count() == 1 {
            format!("char:{key}")
        } else {
            key
        };
        ensure!(
            code.starts_with("char:")
                || matches!(
                    code.as_str(),
                    "enter"
                        | "escape"
                        | "tab"
                        | "backtab"
                        | "space"
                        | "up"
                        | "down"
                        | "left"
                        | "right"
                        | "home"
                        | "end"
                        | "pageup"
                        | "pagedown"
                        | "backspace"
                        | "delete"
                )
                || code
                    .strip_prefix('f')
                    .and_then(|n| n.parse::<u8>().ok())
                    .is_some_and(|n| (1..=24).contains(&n)),
            "Unknown key in {text:?}"
        );
        Ok(Self::event(
            if code == "space" { "char: " } else { &code },
            modifiers,
        ))
    }
}

pub const ACTIONS: &[(&str, &str)] = &[
    ("help", "Keyboard shortcuts"),
    ("workspace-menu", "Workspace menu / all controls"),
    ("project-picker", "Choose project"),
    ("tab-picker", "Choose terminal tab"),
    ("sessions", "Choose session"),
    ("machines", "Saved SSH machines"),
    ("attention", "Attention inbox"),
    ("new-tab", "New terminal tab"),
    ("next-tab", "Next tab"),
    ("previous-tab", "Previous tab"),
    ("next-project", "Next project"),
    ("previous-project", "Previous project"),
    ("new-project", "Add project"),
    ("project-menu", "Project menu"),
    ("pane-menu", "Pane menu / recovery"),
    ("next-pane", "Focus other pane"),
    ("next-panel", "Focus next workspace panel"),
    ("previous-panel", "Focus previous workspace panel"),
    ("split", "Toggle split"),
    ("split-right", "Split pane right"),
    ("split-down", "Split pane down"),
    ("zoom-pane", "Zoom / restore pane"),
    ("swap-pane", "Swap with next pane"),
    ("close-pane", "Close pane, keep terminal"),
    ("focus-left", "Focus pane left"),
    ("focus-right", "Focus pane right"),
    ("focus-up", "Focus pane above"),
    ("focus-down", "Focus pane below"),
    ("move-left", "Move pane left"),
    ("move-right", "Move pane right"),
    ("move-up", "Move pane above"),
    ("move-down", "Move pane below"),
    ("resize-less", "Shrink focused pane"),
    ("resize-more", "Grow focused pane"),
    ("rename-tab", "Rename tab"),
    ("git", "Git status"),
    ("worktrees", "Repository worktrees"),
    ("apps", "App Space"),
    ("terminal", "Terminal Space"),
    ("font-increase", "Increase terminal font"),
    ("font-decrease", "Decrease terminal font"),
    ("font-reset", "Reset terminal font"),
    ("refresh", "Refresh frontend"),
    ("detach", "Detach frontend"),
    ("reload-keys", "Reload keyboard settings"),
    ("history-up", "Scroll terminal history up"),
    ("history-down", "Scroll terminal history down"),
    ("history-end", "Return to live terminal"),
    ("copy-mode", "Select and copy terminal history"),
    ("search-history", "Search terminal history"),
    ("send-tab", "Send literal Tab to terminal/app"),
    ("plugins", "Plugin workflows"),
    ("terminal-links", "Choose a visible terminal URL"),
];
fn valid_action(s: &str) -> bool {
    ACTIONS.iter().any(|(a, _)| *a == s)
        || s.strip_prefix("tab-")
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (1..=9).contains(&n))
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    tab_switches_panels: bool,
    prefix: Option<String>,
    popup_controls: Option<String>,
    bindings: BTreeMap<String, String>,
    direct: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for (key, action) in [
            ("?", "help"),
            ("e", "workspace-menu"),
            ("Shift+p", "project-picker"),
            ("Shift+t", "tab-picker"),
            ("s", "sessions"),
            ("Shift+m", "machines"),
            ("Shift+u", "plugins"),
            ("l", "terminal-links"),
            ("i", "attention"),
            ("c", "new-tab"),
            ("n", "next-tab"),
            ("p", "previous-tab"),
            ("]", "next-project"),
            ("[", "previous-project"),
            ("a", "new-project"),
            ("m", "project-menu"),
            ("x", "pane-menu"),
            ("o", "next-pane"),
            ("v", "split"),
            ("%", "split-right"),
            ("\"", "split-down"),
            ("z", "zoom-pane"),
            ("Shift+o", "swap-pane"),
            ("!", "close-pane"),
            ("Left", "focus-left"),
            ("Right", "focus-right"),
            ("Up", "focus-up"),
            ("Down", "focus-down"),
            ("Alt+Left", "move-left"),
            ("Alt+Right", "move-right"),
            ("Alt+Up", "move-up"),
            ("Alt+Down", "move-down"),
            ("<", "resize-less"),
            (">", "resize-more"),
            ("r", "rename-tab"),
            ("g", "git"),
            ("Shift+w", "worktrees"),
            ("w", "apps"),
            ("t", "terminal"),
            ("=", "font-increase"),
            ("-", "font-decrease"),
            ("0", "font-reset"),
            ("q", "detach"),
            ("Shift+r", "reload-keys"),
            ("PageUp", "history-up"),
            ("PageDown", "history-down"),
            ("End", "history-end"),
            ("/", "search-history"),
            ("Space", "copy-mode"),
            ("Tab", "next-panel"),
            ("Shift+Tab", "previous-panel"),
        ] {
            bindings.insert(key.into(), action.into());
        }
        for n in 1..=9 {
            bindings.insert(n.to_string(), format!("tab-{n}"));
        }
        Self {
            tab_switches_panels: false,
            prefix: Some("Ctrl+b".into()),
            popup_controls: Some("Shift+F6".into()),
            bindings,
            direct: BTreeMap::from([
                ("F1".into(), "help".into()),
                ("F5".into(), "refresh".into()),
                ("Ctrl+Shift+r".into(), "refresh".into()),
            ]),
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Pass,
    Consume,
    ReplayPrefix(String, u8),
    Action(String),
}
pub struct Keyboard {
    settings: Settings,
    prefix: Option<Chord>,
    popup_controls: Option<Chord>,
    bindings: BTreeMap<Chord, String>,
    direct: BTreeMap<Chord, String>,
    pending: bool,
    // Track physical keys, independently of modifier release order.
    consumed: BTreeSet<String>,
}
impl Default for Keyboard {
    fn default() -> Self {
        Self::parse("{}").unwrap()
    }
}
impl Keyboard {
    pub fn popup_controls(&self, code: &str, modifiers: u8) -> bool {
        self.popup_controls.as_ref() == Some(&Chord::event(code, modifiers))
    }
    pub fn popup_controls_hint(&self) -> Option<&str> {
        self.settings.popup_controls.as_deref()
    }
    pub fn tab_switches_panels(&self) -> bool {
        self.settings.tab_switches_panels
    }
    pub fn direct_disabled(&self, code: &str, modifiers: u8) -> bool {
        let chord = Chord::event(code, modifiers);
        self.settings.direct.iter().any(|(key, action)| {
            action == "disabled" && Chord::parse(key).is_ok_and(|candidate| candidate == chord)
        })
    }
    pub fn path() -> PathBuf {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            })
            .join("starflux/keys.json")
    }
    pub fn load() -> Result<Self> {
        match std::fs::read_to_string(Self::path()) {
            Ok(s) => Self::parse(&s).context("Invalid starflux/keys.json"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).context("Read starflux/keys.json"),
        }
    }
    pub fn replace(&mut self, mut keyboard: Self) {
        keyboard.consumed = std::mem::take(&mut self.consumed);
        *self = keyboard;
    }
    fn parse(text: &str) -> Result<Self> {
        // Overlay user entries onto defaults, so changing one shortcut does
        // not silently remove every other binding. "disabled" removes a key.
        let mut value = serde_json::json!({});
        let user: serde_json::Value = serde_json::from_str(text)?;
        ensure!(user.is_object(), "Keyboard settings must be a JSON object");
        let defaults = Settings::default();
        value["bindings"] = serde_json::to_value(&defaults.bindings)?;
        value["direct"] = serde_json::to_value(&defaults.direct)?;
        for (key, v) in user.as_object().unwrap() {
            if matches!(key.as_str(), "bindings" | "direct") && v.is_object() {
                let mut seen = BTreeSet::new();
                for (chord, action) in v.as_object().unwrap() {
                    let parsed = Chord::parse(chord)?;
                    ensure!(seen.insert(parsed.clone()), "Duplicate shortcut {chord:?}");
                    let aliases = value[key]
                        .as_object()
                        .unwrap()
                        .keys()
                        .filter(|old| Chord::parse(old).is_ok_and(|old| old == parsed))
                        .cloned()
                        .collect::<Vec<_>>();
                    for alias in aliases {
                        value[key].as_object_mut().unwrap().remove(&alias);
                    }
                    value[key][chord] = action.clone();
                }
            } else {
                value[key] = v.clone();
            }
        }
        let settings: Settings = serde_json::from_value(value)?;
        let compile =
            |entries: &BTreeMap<String, String>, direct: bool| -> Result<BTreeMap<Chord, String>> {
                let mut out = BTreeMap::new();
                for (key, action) in entries {
                    let chord = Chord::parse(key)?;
                    ensure!(
                        !direct || chord.modifiers != 0 || chord.code.starts_with('f'),
                        "Direct shortcuts require modifiers or a function key"
                    );
                    if action == "disabled" {
                        continue;
                    }
                    ensure!(valid_action(action), "Unknown keyboard action {action:?}");
                    ensure!(
                        out.insert(chord, action.clone()).is_none(),
                        "Duplicate shortcut {key:?}"
                    );
                }
                Ok(out)
            };
        let prefix = settings.prefix.as_deref().map(Chord::parse).transpose()?;
        let popup_controls = settings
            .popup_controls
            .as_deref()
            .map(Chord::parse)
            .transpose()?;
        ensure!(
            popup_controls
                .as_ref()
                .is_none_or(|key| key.modifiers != 0 || key.code.starts_with('f')),
            "Popup controls require modifiers or a function key"
        );
        ensure!(
            prefix.as_ref().is_none_or(|p| p.modifiers != 0),
            "Prefix must include a modifier"
        );
        let bindings = compile(&settings.bindings, false)?;
        let direct = compile(&settings.direct, true)?;
        ensure!(
            prefix.as_ref().is_none_or(|p| !direct.contains_key(p)),
            "Prefix also assigned as a direct shortcut"
        );
        Ok(Self {
            settings,
            prefix,
            popup_controls,
            bindings,
            direct,
            pending: false,
            consumed: BTreeSet::new(),
        })
    }
    pub fn route(
        &mut self,
        code: &str,
        modifiers: u8,
        release: bool,
        repeat: bool,
        blocked: bool,
    ) -> Route {
        let chord = Chord::event(code, modifiers);
        if matches!(
            chord.code.as_str(),
            "shift_left"
                | "shift_right"
                | "control_left"
                | "control_right"
                | "alt_left"
                | "alt_right"
                | "super_left"
                | "super_right"
        ) {
            return Route::Pass;
        }
        if release {
            return if self.consumed.remove(&chord.code) {
                Route::Consume
            } else {
                Route::Pass
            };
        }
        if repeat && self.consumed.contains(&chord.code) {
            return Route::Consume;
        }
        if blocked {
            self.pending = false;
            return Route::Pass;
        }
        let route = if self.pending {
            self.pending = false;
            if chord.code == "escape" {
                Route::Consume
            } else if self.prefix.as_ref() == Some(&chord) {
                Route::Action("literal-prefix".into())
            } else if let Some(action) = self.bindings.get(&chord) {
                Route::Action(action.clone())
            } else {
                let p = self.prefix.as_ref().unwrap();
                Route::ReplayPrefix(p.code.clone(), p.modifiers)
            }
        } else if self.prefix.as_ref() == Some(&chord) {
            self.pending = true;
            Route::Consume
        } else if let Some(action) = self.direct.get(&chord) {
            Route::Action(action.clone())
        } else {
            Route::Pass
        };
        if matches!(route, Route::Consume | Route::Action(_)) {
            self.consumed.insert(chord.code);
        }
        route
    }
    pub fn literal_prefix(&self) -> Option<(String, u8)> {
        self.prefix.as_ref().map(|p| (p.code.clone(), p.modifiers))
    }
    pub fn cancel(&mut self) {
        self.pending = false;
    }
    pub fn pending_hint(&self) -> Option<String> {
        self.pending.then(|| {
            format!(
                "{} · ? shortcuts · Esc cancel",
                self.settings.prefix.as_deref().unwrap_or_default()
            )
        })
    }
    pub fn help(&self) -> Vec<(String, String, String)> {
        let mut rows = vec![];
        for (entries, prefix) in [
            (&self.settings.direct, String::new()),
            (
                &self.settings.bindings,
                self.settings
                    .prefix
                    .as_ref()
                    .map(|p| format!("{p} "))
                    .unwrap_or_default(),
            ),
        ] {
            if std::ptr::eq(entries, &self.settings.bindings) && self.prefix.is_none() {
                continue;
            }
            for (key, action) in entries {
                if action == "disabled" {
                    continue;
                }
                let label = ACTIONS
                    .iter()
                    .find(|(a, _)| *a == action)
                    .map(|(_, l)| l.to_string())
                    .unwrap_or_else(|| format!("Select tab {}", action.trim_start_matches("tab-")));
                rows.push((label, format!("{prefix}{key}"), action.clone()));
            }
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn popup_controls_are_configurable_and_do_not_steal_global_keys() {
        let mut keys = Keyboard::default();
        assert!(keys.popup_controls("f:6", 1));
        assert!(!keys.popup_controls("f:6", 0));
        assert!(!keys.popup_controls("char:b", 2));
        assert_eq!(keys.route("f:6", 1, false, false, false), Route::Pass);
        let keys = Keyboard::parse(r#"{"popup_controls":"Alt+F8"}"#).unwrap();
        assert!(keys.popup_controls("f:8", 4));
        assert!(!keys.popup_controls("f:6", 1));
        assert_eq!(keys.popup_controls_hint(), Some("Alt+F8"));
        let keys = Keyboard::parse(r#"{"popup_controls":null}"#).unwrap();
        assert!(!keys.popup_controls("f:6", 1));
        assert!(keys.popup_controls_hint().is_none());
        assert!(Keyboard::parse(r#"{"popup_controls":"q"}"#).is_err());
    }
    #[test]
    fn terminal_keys_pass_and_unknown_sequences_replay() {
        let mut k = Keyboard::default();
        for (code, m) in [("char:c", 2), ("char:d", 2), ("up", 0), ("char:a", 0)] {
            assert_eq!(k.route(code, m, false, false, false), Route::Pass);
        }
        assert_eq!(k.route("char:b", 2, false, false, false), Route::Consume);
        assert_eq!(
            k.route("char:j", 0, false, false, false),
            Route::ReplayPrefix("char:b".into(), 2)
        );
        assert_eq!(k.route("char:j", 0, true, false, false), Route::Pass);
    }
    #[test]
    fn panel_navigation_requires_prefix_by_default() {
        let mut k = Keyboard::default();
        assert!(!k.tab_switches_panels());
        for (code, modifiers) in [("tab", 0), ("backtab", 1), ("tab", 1)] {
            assert_eq!(k.route(code, modifiers, false, false, false), Route::Pass);
            k.route("char:b", 2, false, false, false);
            assert_eq!(
                k.route(code, modifiers, false, false, false),
                Route::Action(
                    if modifiers == 0 {
                        "next-panel"
                    } else {
                        "previous-panel"
                    }
                    .into()
                )
            );
            k.route(code, modifiers, true, false, false);
        }
        let mut custom = Keyboard::parse(r#"{"prefix":"Ctrl+a"}"#).unwrap();
        assert_eq!(custom.route("char:b", 2, false, false, false), Route::Pass);
        custom.route("char:a", 2, false, false, false);
        assert_eq!(
            custom.route("right", 0, false, false, false),
            Route::Action("focus-right".into())
        );
    }
    #[test]
    fn phases_and_shifted_keys() {
        let mut k = Keyboard::default();
        k.route("char:b", 2, false, false, false);
        assert_eq!(k.route("char:b", 2, false, true, false), Route::Consume);
        assert_eq!(k.route("char:b", 0, true, false, false), Route::Consume);
        assert_eq!(
            k.route("char:?", 0, false, false, false),
            Route::Action("help".into())
        );
        assert_eq!(k.route("char:/", 0, true, false, false), Route::Consume);
        assert_eq!(
            k.route("f:1", 0, false, false, false),
            Route::Action("help".into())
        );
    }
    #[test]
    fn settings_validate_and_preserve_modal_input() {
        assert!(Keyboard::parse(r#"{"direct":{"a":"new-tab"}}"#).is_err());
        assert!(Keyboard::parse(r#"{"prefix":"b"}"#).is_err());
        assert!(Keyboard::parse(r#"{"bindings":{"c":"typo"}}"#).is_err());
        assert!(Keyboard::parse(r#"{"direct":{"F1":"help","f1":"sessions"}}"#).is_err());
        let mut remapped = Keyboard::parse(r#"{"direct":{"f1":"sessions"}}"#).unwrap();
        assert_eq!(
            remapped.route("f:1", 0, false, false, false),
            Route::Action("sessions".into())
        );
        let mut k =
            Keyboard::parse(r#"{"prefix":null,"direct":{"Ctrl+Alt+s":"sessions"}}"#).unwrap();
        assert_eq!(k.route("char:s", 6, false, false, true), Route::Pass);
        assert_eq!(
            k.route("char:s", 6, false, false, false),
            Route::Action("sessions".into())
        );
        assert_eq!(k.route("char:s", 0, true, false, true), Route::Consume);
    }
    #[test]
    fn literal_prefix_and_cancel() {
        let mut k = Keyboard::default();
        k.route("char:b", 2, false, false, false);
        assert_eq!(
            k.route("char:b", 2, false, false, false),
            Route::Action("literal-prefix".into())
        );
        k.route("char:b", 2, false, false, false);
        assert_eq!(k.route("escape", 0, false, false, false), Route::Consume);
        assert_eq!(k.route("char:c", 0, false, false, false), Route::Pass);
        for code in ["space", "char: "] {
            k.route("char:b", 2, false, false, false);
            assert_eq!(
                k.route(code, 0, false, false, false),
                Route::Action("copy-mode".into())
            );
        }
    }
    #[test]
    fn classic_uppercase_modifier_events_and_reload_release() {
        let mut k = Keyboard::default();
        k.route("char:b", 2, false, false, false);
        assert_eq!(k.route("shift_left", 1, false, false, false), Route::Pass);
        assert_eq!(
            k.route("char:R", 0, false, false, false),
            Route::Action("reload-keys".into())
        );
        k.replace(Keyboard::default());
        assert_eq!(k.route("char:r", 0, true, false, false), Route::Consume);
        assert_eq!(k.route("char:r", 0, false, false, false), Route::Pass);
    }
}
