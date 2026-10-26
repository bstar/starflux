//! Pixel-aligned implementation of design/treatments.html's Blueprint study.
use crate::appearance::Appearance;
use starkit::native_surface::{HitRegion, PixelRect, Primitive, Surface};

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
    pub rail: u16,
    pub top: u16,
    pub canvas: PixelRect,
    pub panes: [PixelRect; 2],
    pub frames: [PixelRect; 2],
    pub activity: PixelRect,
    pub theme: PixelRect,
}
impl Layout {
    pub fn new(width: u16, height: u16) -> Self {
        let rail = if width >= 640 { 169 } else { 117 };
        let gap = 14;
        let top = 51;
        let x = (rail + gap).min(width.saturating_sub(4));
        let frame_y = 116.min(height.saturating_sub(100));
        let frame_bottom = height
            .saturating_sub(105)
            .max(frame_y + 30)
            .min(height.saturating_sub(1));
        let canvas = PixelRect::new(
            x + 1,
            (frame_y + 41).min(frame_bottom.saturating_sub(1)),
            width.saturating_sub(x + gap + 3).max(2),
            frame_bottom.saturating_sub(frame_y + 42).max(1),
        );
        let panes = Self::pane_rects(canvas.width, canvas.height);
        let frames = panes.map(|p| {
            PixelRect::new(
                canvas.x + p.x - 1,
                frame_y,
                p.width + 2,
                frame_bottom - frame_y,
            )
        });
        let activity = PixelRect::new(
            x,
            height.saturating_sub(91),
            width.saturating_sub(x + gap + 1),
            44.min(height),
        );
        let theme = PixelRect::new(
            width.saturating_sub(263),
            12,
            170.min(width.saturating_sub(2)),
            27,
        );
        Self {
            width,
            height,
            rail,
            top,
            canvas,
            panes,
            frames,
            activity,
            theme,
        }
    }
    pub fn pane_rects(width: u16, height: u16) -> [PixelRect; 2] {
        let gap = 16.min(width / 4);
        let left = (u32::from(width - gap) * 425 / 1000) as u16;
        [
            PixelRect::new(0, 0, left, height),
            PixelRect::new(left + gap, 0, width - left - gap, height),
        ]
    }
    pub fn session(self, index: usize) -> PixelRect {
        PixelRect::new(13, 98 + index as u16 * 68, self.rail.saturating_sub(26), 62)
    }
    pub fn pane_at(self, x: f64, y: f64) -> Option<usize> {
        self.panes.iter().position(|p| {
            x >= f64::from(p.x)
                && x < f64::from(p.x + p.width)
                && y >= f64::from(p.y)
                && y < f64::from(p.y + p.height)
        })
    }
}
struct Pen {
    surface: Surface,
}
impl Pen {
    fn new(l: Layout, bg: &str) -> Self {
        Self {
            surface: Surface::new(l.width, l.height, bg.into()).with_physical_text(),
        }
    }
    fn rect(&self, r: PixelRect) -> Option<PixelRect> {
        let w = r.width.min(self.surface.width.saturating_sub(r.x));
        let h = r.height.min(self.surface.height.saturating_sub(r.y));
        (w > 0 && h > 0).then(|| PixelRect::new(r.x, r.y, w, h))
    }
    fn fill(&mut self, r: PixelRect, c: &str) {
        if let Some(r) = self.rect(r) {
            self.surface.fill(r, c, 0)
        }
    }
    fn text(&mut self, r: PixelRect, text: impl Into<String>, c: &str, size: u16, bold: bool) {
        if let Some(r) = self.rect(r) {
            self.surface.nodes.push(Primitive::Text {
                rect: r,
                text: text.into(),
                color: c.into(),
                // Original +1 px, then a further +1 pt (rounded to physical pixels).
                size: size.saturating_add(2),
                bold,
                mono: true,
            })
        }
    }
    fn hit(&mut self, r: PixelRect, action: String) {
        if let Some(r) = self.rect(r) {
            self.surface.hits.push(HitRegion { rect: r, action })
        }
    }
    fn border(&mut self, r: PixelRect, c: &str, dashed: bool) {
        if r.width < 2 || r.height < 2 {
            return;
        }
        let step = if self.surface.width > 5000 || self.surface.height > 3000 {
            12
        } else {
            6
        };
        if dashed {
            for x in (0..r.width).step_by(step) {
                for y in [r.y, r.y + r.height - 1] {
                    self.fill(
                        PixelRect::new(r.x + x, y, (step as u16 / 2).min(r.width - x), 1),
                        c,
                    )
                }
            }
            for y in (0..r.height).step_by(step) {
                for x in [r.x, r.x + r.width - 1] {
                    self.fill(
                        PixelRect::new(x, r.y + y, 1, (step as u16 / 2).min(r.height - y)),
                        c,
                    )
                }
            }
        } else {
            self.fill(PixelRect::new(r.x, r.y, r.width, 1), c);
            self.fill(PixelRect::new(r.x, r.y + r.height - 1, r.width, 1), c);
            self.fill(PixelRect::new(r.x, r.y, 1, r.height), c);
            self.fill(PixelRect::new(r.x + r.width - 1, r.y, 1, r.height), c)
        }
    }
    fn grid(&mut self, r: PixelRect, color: &str) {
        for x in
            ((if self.surface.width >= 640 { 169 } else { 117 })..self.surface.width).step_by(17)
        {
            if x >= r.x && x < r.x + r.width {
                self.fill(PixelRect::new(x, r.y, 1, r.height), color)
            }
        }
        for y in (51..self.surface.height).step_by(17) {
            if y >= r.y && y < r.y + r.height {
                self.fill(PixelRect::new(r.x, y, r.width, 1), color)
            }
        }
    }
}
pub struct Model<'a> {
    pub workspace: &'a str,
    pub host: Option<&'a str>,
    pub names: [&'a str; 2],
    pub states: [&'a str; 2],
    pub active: usize,
    pub status: &'a str,
    pub transferring: bool,
    pub hover: Option<usize>,
}
pub fn surfaces(l: Layout, t: &Appearance, m: &Model<'_>) -> (Surface, Surface) {
    let mut p = Pen::new(l, &t.bg);
    let mut fg = Pen::new(l, "transparent");
    p.grid(
        PixelRect::new(
            l.rail,
            l.top,
            l.width.saturating_sub(l.rail),
            l.height.saturating_sub(l.top),
        ),
        &t.grid,
    );
    p.fill(
        PixelRect::new(0, l.top, l.rail, l.height.saturating_sub(l.top)),
        &t.panel,
    );
    p.border(
        PixelRect::new(0, l.top, l.rail, l.height.saturating_sub(l.top)),
        &t.border,
        false,
    );
    p.fill(PixelRect::new(0, 0, l.width, l.top), &t.bg);
    p.border(PixelRect::new(0, 0, l.width, l.top), &t.border, true);
    p.text(
        PixelRect::new(18, 13, 135, 24),
        "S T A R / M U X",
        &t.fg,
        12,
        true,
    );
    if l.width > 700 {
        let title = format!("{} / agent sessions", m.workspace);
        let text_width = (title.chars().count() as u16)
            .saturating_mul(7)
            .min(l.width.saturating_sub(l.rail + 320));
        let center = (120 + l.theme.x) / 2;
        let x = center.saturating_sub(text_width / 2).max(l.rail + 24);
        p.text(
            PixelRect::new(x, 13, l.theme.x.saturating_sub(x + 18), 24),
            title,
            &t.muted,
            11,
            false,
        );
    }
    p.border(l.theme, &t.border, false);
    p.text(
        PixelRect::new(
            l.theme.x + 8,
            l.theme.y + 2,
            l.theme.width.saturating_sub(16),
            23,
        ),
        format!("{} [T]", t.name),
        &t.muted,
        10,
        false,
    );
    p.hit(l.theme, "theme:next".into());
    if l.width > 460 {
        let r = PixelRect::new(l.width - 81, 12, 67, 27);
        p.border(r, &t.border, false);
        p.text(
            PixelRect::new(r.x + 7, r.y + 2, 56, 23),
            "2 sessions",
            &t.muted,
            9,
            false,
        );
    }
    p.text(
        PixelRect::new(13, 66, l.rail - 26, 17),
        "SESSIONS / 02",
        &t.muted,
        9,
        true,
    );
    for (i, name) in m.names.iter().enumerate() {
        let r = l.session(i);
        let selected = i == m.active;
        p.fill(r, if selected { &t.selection } else { &t.raised });
        if selected {
            p.border(r, &t.accent, true)
        }
        let icon = PixelRect::new(r.x + 9, r.y + 10, 18, 18);
        p.fill(icon, &t.selection);
        p.border(icon, &t.accent, false);
        p.text(
            PixelRect::new(icon.x + 3, icon.y, 14, 17),
            if i == 0 { "~" } else { "*" },
            &t.accent,
            11,
            false,
        );
        p.text(
            PixelRect::new(r.x + 33, r.y + 10, r.width.saturating_sub(61), 20),
            *name,
            &t.fg,
            11,
            true,
        );
        p.text(
            PixelRect::new(r.x + r.width - 24, r.y + 10, 18, 20),
            format!("{:02}", i + 1),
            &t.fg,
            10,
            false,
        );
        let state = m.states[i].replace('_', " ");
        let color = if state == "needs input" {
            &t.warning
        } else if state.contains("disconnect") || state == "stopped" {
            &t.error
        } else {
            &t.success
        };
        p.fill(PixelRect::new(r.x + 10, r.y + 42, 4, 4), color);
        p.text(
            PixelRect::new(r.x + 22, r.y + 32, r.width - 30, 22),
            format!(
                "{} / {}",
                state,
                if i == 1 && m.host.is_some() {
                    "SSH"
                } else {
                    "local"
                }
            ),
            &t.muted,
            9,
            false,
        );
        p.hit(r, format!("pane:{i}"));
    }
    // Connection information stays near the bottom, as in the reference rail.
    let connection_y = l.height.saturating_sub(155).max(258);
    p.text(
        PixelRect::new(13, connection_y, l.rail - 26, 18),
        "CONNECTION",
        &t.muted,
        9,
        true,
    );
    p.text(
        PixelRect::new(13, connection_y + 24, l.rail - 26, 20),
        m.host.unwrap_or("Local machine"),
        &t.fg,
        10,
        false,
    );
    p.fill(
        PixelRect::new(13, connection_y + 56, l.rail - 26, 1),
        &t.border,
    );
    p.text(
        PixelRect::new(13, connection_y + 65, l.rail - 26, 20),
        "Ctrl+Esc / keys",
        &t.muted,
        9,
        false,
    );
    p.text(
        PixelRect::new(13, connection_y + 86, l.rail - 26, 20),
        "q / close workspace",
        &t.muted,
        9,
        false,
    );
    p.text(
        PixelRect::new(l.canvas.x, 66, 190.min(l.canvas.width), 20),
        "Workspace",
        &t.fg,
        13,
        true,
    );
    p.text(
        PixelRect::new(l.canvas.x, 88, 320.min(l.canvas.width), 17),
        "Persistent sessions / local + remote",
        &t.muted,
        9,
        false,
    );
    if l.canvas.width > 450 {
        let r = PixelRect::new(l.width - 110, 71, 82, 24);
        p.fill(r, &t.raised);
        p.text(
            PixelRect::new(r.x + 9, r.y + 1, 72, 22),
            "Terminals",
            &t.fg,
            10,
            false,
        );
    }
    for i in 0..2 {
        let frame = l.frames[i];
        p.fill(PixelRect::new(frame.x, frame.y, frame.width, 41), &t.panel);
        p.text(
            PixelRect::new(
                frame.x + 10,
                frame.y + 9,
                frame.width.saturating_sub(100),
                22,
            ),
            format!("{:02} / {}", i + 1, m.names[i]),
            &t.fg,
            10,
            false,
        );
        let host = if i == 0 {
            "LOCAL"
        } else if m.host.is_some() {
            "SSH"
        } else {
            "LOCAL"
        };
        p.text(
            PixelRect::new(
                frame.x + frame.width.saturating_sub(76),
                frame.y + 9,
                66,
                22,
            ),
            host,
            &t.success,
            9,
            false,
        );
        p.hit(
            PixelRect::new(frame.x, frame.y, frame.width, 41),
            format!("pane:{i}"),
        );
        fg.fill(
            PixelRect::new(frame.x, frame.y + 40, frame.width, 1),
            &t.border,
        );
    }
    let gap = PixelRect::new(
        l.canvas.x + l.panes[0].width,
        l.canvas.y,
        l.panes[1].x - l.panes[0].width,
        l.canvas.height,
    );
    fg.fill(gap, &t.bg);
    fg.grid(gap, &t.grid);
    for frame in l.frames {
        fg.border(frame, &t.border, true);
    }
    let focused = l.frames[m.active.min(1)];
    let ring = PixelRect::new(
        focused.x.saturating_sub(3),
        focused.y.saturating_sub(3),
        focused.width + 6,
        focused.height + 6,
    );
    // Two background pixels separate the outer one-pixel focus rule from chrome.
    for inset in 1..=2 {
        fg.border(
            PixelRect::new(
                focused.x - inset,
                focused.y - inset,
                focused.width + inset * 2,
                focused.height + inset * 2,
            ),
            &t.bg,
            false,
        )
    }
    fg.border(ring, &t.accent, false);
    fg.border(focused, &t.accent, false);
    if let Some(i) = m.hover {
        let f = l.frames[i.min(1)];
        for (x, y) in [
            (f.x + 2, f.y + 2),
            (f.x + f.width.saturating_sub(18), f.y + 2),
            (f.x + 2, f.y + f.height.saturating_sub(4)),
            (
                f.x + f.width.saturating_sub(18),
                f.y + f.height.saturating_sub(4),
            ),
        ] {
            fg.fill(PixelRect::new(x, y, 16, 2), &t.accent);
        }
        let badge = PixelRect::new(f.x + 12, f.y + 43, f.width.saturating_sub(24), 38);
        fg.fill(badge, &t.panel);
        fg.border(badge, &t.accent, true);
        fg.text(
            PixelRect::new(badge.x + 9, badge.y + 7, badge.width.saturating_sub(18), 24),
            format!(
                "Copy to {} / {}",
                m.names[i],
                if i == 1 {
                    m.host.unwrap_or("local")
                } else {
                    "local"
                }
            ),
            &t.accent,
            11,
            true,
        );
    }
    p.fill(l.activity, &t.panel);
    p.border(l.activity, &t.border, true);
    p.text(
        PixelRect::new(
            l.activity.x + 10,
            l.activity.y + 3,
            l.activity.width.saturating_sub(130),
            19,
        ),
        if m.transferring {
            "File transfer"
        } else {
            "Session activity"
        },
        &t.fg,
        10,
        false,
    );
    p.text(
        PixelRect::new(
            l.activity.x + 10,
            l.activity.y + 21,
            l.activity.width.saturating_sub(130),
            18,
        ),
        m.status,
        &t.muted,
        9,
        false,
    );
    if m.transferring {
        let r = PixelRect::new(
            l.activity.x + l.activity.width.saturating_sub(96),
            l.activity.y + 10,
            84,
            24,
        );
        p.border(r, &t.warning, false);
        p.text(
            PixelRect::new(r.x + 10, r.y + 1, 70, 22),
            "Cancel",
            &t.warning,
            10,
            false,
        );
        p.hit(r, "cancel-transfer".into());
    }
    let y = l.height.saturating_sub(33);
    p.fill(PixelRect::new(0, y, l.width, 33), &t.panel);
    p.fill(PixelRect::new(0, y, l.width, 1), &t.border);
    p.text(
        PixelRect::new(18, y + 6, 195.min(l.width.saturating_sub(18)), 22),
        "Sessions retained",
        &t.muted,
        9,
        false,
    );
    if l.width > 650 {
        p.text(
            PixelRect::new(l.rail + 18, y + 6, l.width.saturating_sub(l.rail + 175), 22),
            "Drop onto a pane / no automatic submission",
            &t.muted,
            9,
            false,
        );
        p.text(
            PixelRect::new(l.width - 151, y + 6, 135, 22),
            "Ctrl+Esc / release",
            &t.muted,
            9,
            false,
        );
    }
    fg.border(PixelRect::new(0, 0, l.width, l.height), &t.border, false);
    (p.surface, fg.surface)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_geometry_matches_the_measured_html_reference() {
        let l = Layout::new(1200, 700);
        assert_eq!(
            l.frames,
            [
                PixelRect::new(183, 116, 420, 479),
                PixelRect::new(617, 116, 568, 479)
            ]
        );
        assert_eq!(l.canvas, PixelRect::new(184, 157, 1000, 437));
        assert_eq!(l.activity, PixelRect::new(183, 609, 1002, 44));
    }
    #[test]
    fn surfaces_stay_bounded_and_use_square_edges() {
        let t = Appearance::load("kanagawa").unwrap();
        for (w, h) in [
            (640, 480),
            (1200, 700),
            (1500, 1900),
            (3840, 2160),
            (8192, 3900),
        ] {
            let l = Layout::new(w, h);
            let (bg, fg) = surfaces(
                l,
                &t,
                &Model {
                    workspace: "test",
                    host: Some("atlas"),
                    names: ["Shell", "Codex"],
                    states: ["running", "needs_input"],
                    active: 1,
                    status: "Ready",
                    transferring: true,
                    hover: Some(1),
                },
            );
            bg.validate()
                .unwrap_or_else(|e| panic!("{w}x{h} background nodes={} {e}", bg.nodes.len()));
            fg.validate()
                .unwrap_or_else(|e| panic!("{w}x{h} foreground nodes={} {e}", fg.nodes.len()));
            assert!(bg.nodes.iter().chain(&fg.nodes).all(|node|!matches!(node,Primitive::Fill{radius,..}|Primitive::Border{radius,..}if *radius!=0)));
            assert_eq!(l.pane_at(1., 1.), Some(0));
            assert_eq!(l.pane_at(f64::from(l.panes[1].x), 1.), Some(1));
            assert_eq!(l.pane_at(f64::from(l.panes[0].width + 1), 1.), None);
        }
    }
}
