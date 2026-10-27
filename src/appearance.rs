//! Blueprint's semantic palette, matching the HTML treatment's byte-space mixes.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use starkit::theme::{Theme, ThemeFile, Variant, color::Rgb};

pub fn default_theme() -> String {
    "kanagawa".into()
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Appearance {
    pub id: String,
    pub name: String,
    pub bg: String,
    pub panel: String,
    pub raised: String,
    pub fg: String,
    pub muted: String,
    pub border: String,
    pub accent: String,
    pub selection: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    pub grid: String,
    pub ansi: Vec<String>,
}
fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    let c = |x: u8, y: u8| (f64::from(x) * (1. - amount) + f64::from(y) * amount).round() as u8;
    Rgb::new(c(a.r, b.r), c(a.g, b.g), c(a.b, b.b))
}
fn readable(color: Rgb, surfaces: &[Rgb], target: f64) -> Rgb {
    let white = Rgb::new(255, 255, 255);
    let black = Rgb::new(0, 0, 0);
    let end = if surfaces[0].contrast(black) > surfaces[0].contrast(white) {
        black
    } else {
        white
    };
    for i in 0..=100 {
        let c = mix(color, end, f64::from(i) / 100.);
        if surfaces.iter().all(|b| c.contrast(*b) >= target) {
            return c;
        }
    }
    end
}
impl Appearance {
    pub fn load(id: &str) -> Result<Self> {
        let file = if id == "system" {
            starkit::theme::system::theme()
                .map(|(file, _)| file)
                .unwrap_or(ThemeFile::parse(
                    starkit::theme::BUILTINS
                        .iter()
                        .find(|b| b.id == "kanagawa")
                        .unwrap()
                        .toml,
                )?)
        } else if let Some(b) = starkit::theme::BUILTINS.iter().find(|b| b.id == id) {
            ThemeFile::parse(b.toml)?
        } else {
            bail!("Unknown theme '{id}'. Run starmux --themes for available palettes")
        };
        Self::from_file(id, &file)
    }
    fn from_file(id: &str, file: &ThemeFile) -> Result<Self> {
        let core = Theme::resolve(file);
        let b = file.base16;
        let bg = b.map_or(core.bg, |b| b.base00);
        let panel = b.map_or(core.panel_bg, |b| b.base01);
        let base_fg = b.map_or(core.fg, |b| b.base05);
        let raised = mix(
            panel,
            Rgb::new(255, 255, 255),
            if file.meta.variant == Variant::Light {
                0.48
            } else {
                0.06
            },
        );
        let surfaces = [bg, panel, raised];
        let fg = readable(base_fg, &surfaces, 4.5);
        let accent = readable(b.map_or(core.accent, |b| b.base0D), &surfaces, 4.5);
        let raw = if let Some(b) = b {
            vec![
                b.base00, b.base08, b.base0B, b.base0A, b.base0D, b.base0E, b.base0C, b.base05,
                b.base03, b.base08, b.base0B, b.base0A, b.base0D, b.base0E, b.base0C, b.base07,
            ]
        } else {
            vec![
                core.bg,
                core.error,
                core.ok,
                core.warn,
                core.accent,
                core.accent,
                core.ok,
                core.fg,
                core.dim,
                core.error,
                core.ok,
                core.warn,
                core.accent,
                core.accent,
                core.ok,
                core.fg,
            ]
        };
        Ok(Self {
            id: id.into(),
            name: if id == "system" {
                format!("System / {}", core.name)
            } else {
                core.name
            },
            bg: bg.to_hex(),
            panel: panel.to_hex(),
            raised: raised.to_hex(),
            fg: fg.to_hex(),
            muted: readable(mix(bg, readable(base_fg, &[bg], 4.5), 0.58), &surfaces, 4.5).to_hex(),
            border: readable(mix(bg, readable(base_fg, &[bg], 4.5), 0.19), &[bg], 1.35).to_hex(),
            accent: accent.to_hex(),
            selection: mix(
                bg,
                accent,
                if file.meta.variant == Variant::Light {
                    0.10
                } else {
                    0.16
                },
            )
            .to_hex(),
            success: readable(b.map_or(core.ok, |b| b.base0B), &surfaces, 4.5).to_hex(),
            warning: readable(b.map_or(core.warn, |b| b.base0A), &surfaces, 4.5).to_hex(),
            error: readable(b.map_or(core.error, |b| b.base08), &surfaces, 4.5).to_hex(),
            grid: mix(bg, accent, 0.03).to_hex(),
            ansi: raw
                .into_iter()
                .enumerate()
                .map(|(i, c)| {
                    let c = match i {
                        0 => bg,
                        1 | 9 => readable(b.map_or(core.error, |b| b.base08), &surfaces, 4.5),
                        2 | 10 => readable(b.map_or(core.ok, |b| b.base0B), &surfaces, 4.5),
                        3 | 11 => readable(b.map_or(core.warn, |b| b.base0A), &surfaces, 4.5),
                        4 | 12 => accent,
                        7 | 15 => fg,
                        _ => readable(c, &surfaces, 4.5),
                    };
                    c.to_hex()
                })
                .collect(),
        })
    }
    pub fn tab_color(&self) -> String {
        let bg = Rgb::parse_hex(&self.bg).unwrap();
        let level = if bg.contrast(Rgb::new(0, 0, 0)) > bg.contrast(Rgb::new(255, 255, 255)) {
            96
        } else {
            176
        };
        Rgb::new(level, level, level).to_hex()
    }
    pub fn activity_color(&self, base: &str, strength: f64) -> String {
        self.activity_tint(base, &self.accent, strength)
    }
    pub fn attention_color(&self, base: &str, strength: f64) -> String {
        self.activity_tint(base, &self.warning, strength)
    }
    fn activity_tint(&self, base: &str, accent: &str, strength: f64) -> String {
        let base = Rgb::parse_hex(base).unwrap();
        let crest = readable(
            mix(
                Rgb::parse_hex(accent).unwrap(),
                Rgb::parse_hex(&self.fg).unwrap(),
                0.65,
            ),
            &[Rgb::parse_hex(&self.bg).unwrap()],
            4.5,
        );
        mix(base, crest, strength.clamp(0., 1.)).to_hex()
    }
    pub fn kitty_colors(&self) -> Vec<String> {
        let mut colors = vec![
            format!("background={}", self.bg),
            format!("foreground={}", self.fg),
            format!("cursor={}", self.accent),
            format!("cursor_text_color={}", self.bg),
            format!("selection_background={}", self.selection),
            format!("selection_foreground={}", self.fg),
        ];
        colors.extend(
            self.ansi
                .iter()
                .enumerate()
                .map(|(i, c)| format!("color{i}={c}")),
        );
        colors
    }
    pub fn cycle(&self) -> Result<Self> {
        let list = &starkit::theme::BUILTINS;
        let at = list.iter().position(|b| b.id == self.id).unwrap_or(0);
        Self::load(list[(at + 1) % list.len()].id).context("Load next theme")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_palette_has_readable_semantic_text() {
        for b in starkit::theme::BUILTINS {
            let t = Appearance::load(b.id).unwrap();
            for surface in [&t.bg, &t.panel, &t.raised] {
                for text in [&t.fg, &t.muted, &t.accent, &t.success, &t.warning, &t.error] {
                    assert!(
                        Rgb::parse_hex(text)
                            .unwrap()
                            .contrast(Rgb::parse_hex(surface).unwrap())
                            >= 4.49,
                        "{} {text} {surface}",
                        b.id
                    );
                }
            }
            for strength in [0., 0.25, 0.5, 0.75, 1.] {
                for base in [&t.fg, &t.accent] {
                    let wave = Rgb::parse_hex(&t.activity_color(base, strength)).unwrap();
                    assert!(
                        wave.contrast(Rgb::parse_hex(&t.bg).unwrap()) >= 4.49,
                        "{} activity text is too faint at {}",
                        b.id,
                        strength
                    );
                }
            }
            assert_eq!(t.ansi.len(), 16);
        }
    }
    #[test]
    fn kanagawa_matches_gallery_tokens() {
        let t = Appearance::load("kanagawa").unwrap();
        assert_eq!(t.bg, "#1f1f28");
        assert_eq!(t.panel, "#2a2a37");
        assert_eq!(t.grid, "#22232d");
        assert!(Appearance::load("../../bad").is_err());
    }
}
