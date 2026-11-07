use super::*;
const META: &str = "starflux-font-v1";
#[derive(Default)]
pub(super) struct Fonts {
    pub family: Option<String>,
    pub families: Vec<String>,
    pub available: bool,
    pub pending_menu: bool,
}
impl Workspace {
    pub(super) fn font_notice(&mut self, message: &str) -> bool {
        let Some(raw) = message.strip_prefix("starflux-font-v1:") else {
            return false;
        };
        if raw.len() > 128 * 1024 {
            return true;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
            return true;
        };
        if let Some(error) = value["error"].as_str() {
            self.note(format!(
                "Font change failed: {}",
                error.chars().take(180).collect::<String>()
            ));
            return true;
        }
        let clean = |s: &str| !s.is_empty() && s.len() <= 192 && !s.chars().any(char::is_control);
        let Some(family) = value["family"].as_str().filter(|s| clean(s)) else {
            return true;
        };
        let Some(size) = value["millipoints"]
            .as_u64()
            .filter(|s| (4000..=96000).contains(s))
        else {
            return true;
        };
        self.fonts.family = Some(family.into());
        self.fonts.available = true;
        if value["append"] != true {
            self.fonts.families.clear();
        }
        self.fonts.families.extend(
            value["families"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().filter(|s| clean(s)).map(String::from))
                .take(512usize.saturating_sub(self.fonts.families.len())),
        );
        self.native_font_points = Some(size as u32);
        if self.initial_native_font_points.is_none() {
            self.initial_native_font_points = self.native_font_points;
        }
        self.native_layout_key.clear();
        self.note(format!("Font: {family} · {:.1}pt", size as f64 / 1000.));
        if self.fonts.pending_menu && value["complete"] != false {
            self.fonts.pending_menu = false;
            self.open_font_menu();
        }
        true
    }
    fn open_font_menu(&mut self) {
        use crate::shelf::{ContextMenu, MenuItem};
        let mut items = vec![MenuItem {
            label: "Reset font size".into(),
            hint: String::new(),
            action: "terminal:font:reset".into(),
        }];
        items.extend(
            self.fonts
                .families
                .iter()
                .enumerate()
                .map(|(i, f)| MenuItem {
                    label: f.clone(),
                    hint: if self.fonts.family.as_ref() == Some(f) {
                        "Selected".into()
                    } else {
                        String::new()
                    },
                    action: format!("terminal:font:family:{i}"),
                }),
        );
        self.release();
        self.context_menu = Some(ContextMenu {
            position: [self.viewport.width.saturating_sub(650) as u16, 120],
            title: "Monospace fonts".into(),
            subtitle: format!(
                "{} · Family applies to all Kitty windows",
                self.native_font_points
                    .map_or("".into(), |s| format!("{:.1}pt", s as f64 / 1000.))
            ),
            items,
            selected: Some(0),
        });
    }
    pub(super) fn font_action(&mut self, action: &str) -> Result<bool> {
        if action == "terminal:font:select" {
            ensure!(!self.cells, "Cells mode inherits the terminal font");
            if self.fonts.available {
                self.open_font_menu();
            } else {
                self.fonts.pending_menu = true;
                self.osc(META, Some(json!({"op":"probe"}).to_string()));
                self.note("Reading fonts on the display computer. The current local frontend and STAR/FLUX font watcher are required.".into());
            }
            return Ok(true);
        }
        if let Some(index) = action.strip_prefix("terminal:font:family:") {
            let font = self
                .fonts
                .families
                .get(index.parse::<usize>()?)
                .context("Font list changed; reopen the picker")?
                .clone();
            ensure!(self.fonts.available, "Local font controls are unavailable");
            self.osc(
                META,
                Some(json!({"op":"family", "family":font}).to_string()),
            );
            return Ok(true);
        }
        if self.desktop.is_none()
            && (self.fonts.available || self.native_font_points.is_none())
            && matches!(
                action,
                "terminal:font:increase" | "terminal:font:decrease" | "terminal:font:reset"
            )
        {
            ensure!(!self.cells, "Cells mode inherits the terminal font");
            let request = match action {
                "terminal:font:increase" => json!({"op":"size","delta":1000}),
                "terminal:font:decrease" => json!({"op":"size","delta":-1000}),
                _ => json!({"op":"size","size":0}),
            };
            self.osc(META, Some(request.to_string()));
            return Ok(true);
        }
        Ok(false)
    }
}
