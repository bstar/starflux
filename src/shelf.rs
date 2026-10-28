//! App Shelf: single pixel borders, drafting grid, project spaces and app cards.
use crate::{appearance::Appearance, project_state::Space};
use starkit::native_surface::{HitRegion, PixelRect as R, Primitive, Surface};

const TAB_HEIGHT: u16 = 31;
const TAB_LEAD: u16 = 8;
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
    pub sidebar: R,
    pub frame: R,
    pub canvas: R,
    pub shelf: R,
    pub tabs: R,
    pub footer: R,
}
impl Layout {
    pub fn new(width: u16, height: u16, space: Space) -> Self {
        let side = if width >= 1100 {
            252
        } else if width >= 800 {
            224
        } else if width >= 600 {
            188
        } else {
            144.min(width / 2)
        };
        let mut layout = Self::body(width.saturating_sub(side).max(1), height, space);
        layout.width = width;
        layout.sidebar = R::new(0, 0, side, height);
        for r in [
            &mut layout.frame,
            &mut layout.canvas,
            &mut layout.shelf,
            &mut layout.tabs,
            &mut layout.footer,
        ] {
            r.x += side;
        }
        layout
    }
    fn body(width: u16, height: u16, space: Space) -> Self {
        let margin = if width < 600 { 8 } else { 14 };
        let x = margin;
        let w = width.saturating_sub(margin * 2).max(1);
        let y = 90.min(height.saturating_sub(100));
        let shelf_h = 0;
        let bottom = height.saturating_sub(40 + shelf_h);
        let frame = R::new(x, y, w, bottom.saturating_sub(y).max(1));
        let tab_h = if space != Space::Apps { 34 } else { 0 };
        let mut canvas = R::new(
            x + 1,
            y + 37 + tab_h,
            w.saturating_sub(2).max(1),
            frame
                .height
                .saturating_sub(38 + tab_h + if space == Space::Apps { 34 } else { 0 })
                .max(1),
        );
        if space == Space::Apps {
            canvas = crate::app_view::canvas(canvas);
        }
        Self {
            width,
            height,
            sidebar: R::default(),
            frame,
            canvas,
            shelf: R::new(x, bottom.saturating_add(10), w, shelf_h.saturating_sub(12)),
            tabs: R::new(x + 1, y + 37, w.saturating_sub(2), tab_h),
            footer: R::new(0, height.saturating_sub(28), width, 28),
        }
    }
}
#[derive(serde::Serialize)]
pub struct ProjectTab<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub host: &'a str,
    pub working: usize,
    pub waiting: usize,
    pub running: usize,
    pub questions: usize,
}
#[derive(Clone, Copy, serde::Serialize)]
pub struct DesktopMap<'a> {
    pub size: [u32; 2],
    pub windows: &'a [crate::project::WindowLayout],
    pub selected: Option<usize>,
}
#[derive(serde::Serialize)]
pub struct AppCard<'a> {
    pub action: String,
    pub name: &'a str,
    pub selected: bool,
}
#[derive(serde::Serialize)]
pub struct ModalField<'a> {
    pub label: &'a str,
    pub text: &'a str,
    pub cursor: usize,
}
#[derive(serde::Serialize)]
pub struct ApplicationPicker<'a> {
    pub query: &'a str,
    pub cursor: usize,
    pub focused: bool,
    pub status: &'a str,
    pub selection_name: Option<&'a str>,
    pub selected: usize,
    pub count: usize,
    pub rows: Vec<(usize, &'a str, &'a str)>,
}
#[derive(serde::Serialize)]
pub struct Modal<'a> {
    pub applications: Option<ApplicationPicker<'a>>,
    pub catalog_available: bool,
    pub title: &'a str,
    pub fields: Vec<ModalField<'a>>,
    pub selected: usize,
    pub error: &'a str,
}
#[derive(Clone, serde::Serialize)]
pub struct MenuItem {
    pub label: String,
    pub hint: String,
    pub action: String,
}
#[derive(serde::Serialize)]
pub struct ContextMenu {
    pub position: [u16; 2],
    pub title: String,
    pub subtitle: String,
    pub items: Vec<MenuItem>,
    pub selected: Option<usize>,
}
impl ContextMenu {
    pub fn bounds(&self, width: u16, height: u16) -> R {
        let w = 288.min(width.saturating_sub(12));
        let h = (88 + self.items.len() as u16 * 34).min(height.saturating_sub(12));
        R::new(
            self.position[0].min(width.saturating_sub(w + 6)),
            self.position[1].min(height.saturating_sub(h + 6)),
            w,
            h,
        )
    }
}
#[derive(serde::Serialize)]
pub struct Model<'a> {
    pub app_view: bool,
    pub project: &'a str,
    pub project_label: &'a str,
    pub project_offset: usize,
    pub project_total: usize,
    pub sort_hover: bool,
    pub sort_mode_hover: bool,
    pub project_manual: bool,
    pub project_drag: Option<(&'a str, Option<usize>)>,
    pub project_sort_high: bool,
    pub wave_phase: u16,
    pub reduced_motion: bool,
    pub directory: &'a str,
    pub host: &'a str,
    pub theme: &'a str,
    pub audio_label: &'a str,
    pub space: Space,
    pub projects: Vec<ProjectTab<'a>>,
    pub terminals: Vec<(usize, String)>,
    pub terminal_status: std::collections::BTreeMap<usize, String>,
    pub terminal: usize,
    pub terminal_font_size: String,
    pub split: bool,
    pub agent_name: &'a str,
    pub agent_state: &'a str,
    pub status: &'a str,
    pub apps: Vec<AppCard<'a>>,
    pub app_desktop: Option<DesktopMap<'a>>,
    pub connected: bool,
    pub desktop_host: &'a str,
    pub context_menu: Option<&'a ContextMenu>,
    pub git: Option<&'a crate::git::Status>,
    pub git_open: bool,
    pub git_scroll: usize,
    pub modal: Option<Modal<'a>>,
    pub transferring: bool,
    pub hover: bool,
}
struct Pen {
    surface: Surface,
}
impl Pen {
    fn new(w: u16, h: u16, bg: &str) -> Self {
        Self {
            surface: Surface::new(w, h, bg.into()).with_physical_text(),
        }
    }
    fn clip(&self, r: R) -> Option<R> {
        let w = r.width.min(self.surface.width.saturating_sub(r.x));
        let h = r.height.min(self.surface.height.saturating_sub(r.y));
        (w > 0 && h > 0).then(|| R::new(r.x, r.y, w, h))
    }
    fn fill(&mut self, r: R, c: &str) {
        if let Some(r) = self.clip(r) {
            self.surface.fill(r, c, 0)
        }
    }
    fn text(&mut self, r: R, text: impl Into<String>, c: &str, size: u16, bold: bool) {
        if let Some(r) = self.clip(r) {
            let mut text = text.into();
            if text.len() > 4096 {
                let mut end = 4096;
                while !text.is_char_boundary(end) {
                    end -= 1
                }
                text.truncate(end);
            }
            self.surface.nodes.push(Primitive::Text {
                rect: r,
                text,
                color: c.into(),
                size,
                bold,
                mono: true,
            })
        }
    }
    fn centered_text(&mut self, r: R, text: impl Into<String>, color: &str, size: u16, bold: bool) {
        let node = self.surface.nodes.len();
        self.text(r, text, color, size, bold);
        if self.surface.nodes.len() > node {
            self.surface.centered_text.push(node as u16);
        }
    }
    fn border(&mut self, r: R, c: &str) {
        if r.width < 2 || r.height < 2 {
            return;
        }
        self.fill(R::new(r.x, r.y, r.width, 1), c);
        self.fill(R::new(r.x, r.y + r.height - 1, r.width, 1), c);
        self.fill(R::new(r.x, r.y, 1, r.height), c);
        self.fill(R::new(r.x + r.width - 1, r.y, 1, r.height), c)
    }
    fn hit(&mut self, r: R, action: impl Into<String>) {
        if let Some(r) = self.clip(r) {
            self.surface.hits.push(HitRegion {
                rect: r,
                action: action.into(),
            })
        }
    }
    fn button(
        &mut self,
        r: R,
        text: &str,
        action: impl Into<String>,
        t: &Appearance,
        selected: bool,
    ) {
        self.fill(r, if selected { &t.selection } else { &t.panel });
        self.border(r, if selected { &t.accent } else { &t.border });
        self.centered_text(
            R::new(
                r.x + 2,
                r.y + 2,
                r.width.saturating_sub(4),
                r.height.saturating_sub(4),
            ),
            text,
            if selected { &t.accent } else { &t.fg },
            if matches!(text, "−" | "+") { 20 } else { 12 },
            false,
        );
        self.hit(r, action)
    }
    fn tab(&mut self, r: R, text: &str, action: impl Into<String>, t: &Appearance, selected: bool) {
        // The active tab interrupts the shared baseline with a square, open-bottom
        // outline. All labels share the same centered text box and neutral ink.
        let ink = t.tab_color();
        if selected {
            self.fill(r, &t.bg);
            self.fill(R::new(r.x, r.y, r.width, 1), &ink);
            self.fill(R::new(r.x, r.y, 1, r.height), &ink);
            self.fill(R::new(r.x + r.width - 1, r.y, 1, r.height), &ink);
        }
        let inset = 2;
        self.centered_text(
            R::new(
                r.x + 12,
                r.y + inset,
                r.width.saturating_sub(24),
                r.height.saturating_sub(4),
            ),
            text,
            &ink,
            12,
            false,
        );
        self.hit(r, action);
    }
    fn grid(&mut self, r: R, c: &str) {
        for x in (r.x..r.x + r.width).step_by(24) {
            self.fill(R::new(x, r.y, 1, r.height), c)
        }
        for y in (r.y..r.y + r.height).step_by(24) {
            self.fill(R::new(r.x, y, r.width, 1), c)
        }
    }
}
// Fit the complete desktop, then clip windows in source coordinates before
// projecting; partly offscreen windows must not spill into labels or borders.
fn desktop_bounds(area: R, size: [u32; 2]) -> Option<R> {
    if area.width < 2 || area.height < 2 || size.contains(&0) {
        return None;
    }
    let scale = (f64::from(area.width) / f64::from(size[0]))
        .min(f64::from(area.height) / f64::from(size[1]));
    let width = (f64::from(size[0]) * scale).round().max(1.) as u16;
    let height = (f64::from(size[1]) * scale).round().max(1.) as u16;
    Some(R::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    ))
}
fn map_window(desktop: R, size: [u32; 2], window: &crate::project::WindowLayout) -> Option<R> {
    if size.contains(&0) || window.width <= 0 || window.height <= 0 {
        return None;
    }
    let x = i64::from(window.x).clamp(0, i64::from(size[0]));
    let y = i64::from(window.y).clamp(0, i64::from(size[1]));
    let right = (i64::from(window.x) + i64::from(window.width)).clamp(0, i64::from(size[0]));
    let bottom = (i64::from(window.y) + i64::from(window.height)).clamp(0, i64::from(size[1]));
    if right <= x || bottom <= y {
        return None;
    }
    let px =
        |value: i64| (value as f64 * f64::from(desktop.width) / f64::from(size[0])).round() as u16;
    let py =
        |value: i64| (value as f64 * f64::from(desktop.height) / f64::from(size[1])).round() as u16;
    Some(R::new(
        desktop.x + px(x),
        desktop.y + py(y),
        (px(right) - px(x)).max(1),
        (py(bottom) - py(y)).max(1),
    ))
}
fn desktop_map(p: &mut Pen, area: R, map: Option<DesktopMap<'_>>, t: &Appearance) {
    let size = map.map_or([16, 9], |m| m.size);
    let Some(bounds) = desktop_bounds(area, size) else {
        return;
    };
    p.grid(bounds, &t.grid);
    p.border(bounds, &t.border);
    let Some(map) = map else {
        return;
    };
    // Draw the selected window last so its outline remains visible in overlaps.
    for selected in [false, true] {
        for (index, window) in map.windows.iter().enumerate() {
            let active = map.selected == Some(index);
            if active != selected {
                continue;
            }
            if let Some(rect) = map_window(bounds, size, window) {
                p.fill(rect, if active { &t.selection } else { &t.panel });
                p.border(rect, if active { &t.accent } else { &t.muted });
                if rect.width > 6 && rect.height > 8 {
                    p.fill(
                        R::new(rect.x + 2, rect.y + 3, rect.width - 4, 1),
                        if active { &t.accent } else { &t.border },
                    );
                }
            }
        }
    }
}
pub fn project_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(198) / 34).max(1)
}
fn activity_icon(
    p: &mut Pen,
    position: [u16; 2],
    t: &Appearance,
    phase: u16,
    reduced_motion: bool,
    attention: bool,
    working: bool,
) {
    let [x, y] = position;
    // A fixed central spark and eight orbit points. Only their light moves;
    // integer geometry stays crisp at the sidebar's native pixel size.
    let phase = if reduced_motion {
        0.
    } else {
        f64::from(phase % 48)
    };
    for (i, (dx, dy)) in [
        (7, 0),
        (12, 2),
        (14, 7),
        (12, 12),
        (7, 14),
        (2, 12),
        (0, 7),
        (2, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let behind = (phase - i as f64 * 6.).rem_euclid(48.);
        // Fade the tail over most of an orbit and ease the next point in.
        let strength = if behind > 42. {
            (behind - 42.) / 6.
        } else {
            (1. - behind / 36.).max(0.).powf(1.5)
        };
        p.fill(
            R::new(x + dx, y + dy, 2, 2),
            &if attention {
                t.attention_color(&t.border, strength)
            } else {
                t.activity_color(&t.border, strength * if working { 1. } else { 0.55 })
            },
        );
    }
    let ink = if attention { &t.warning } else { &t.accent };
    p.fill(R::new(x + 7, y + 5, 2, 6), ink);
    p.fill(R::new(x + 5, y + 7, 6, 2), ink);
    p.fill(
        R::new(x + 7, y + 7, 2, 2),
        &if attention {
            t.attention_color(ink, 0.85)
        } else {
            t.activity_color(ink, 0.85)
        },
    );
}
fn move_surface(surface: &mut Surface, x: u16, width: u16) {
    surface.width = width;
    for node in &mut surface.nodes {
        let r = match node {
            Primitive::Text { rect, .. }
            | Primitive::Fill { rect, .. }
            | Primitive::Border { rect, .. } => rect,
            _ => continue,
        };
        r.x += x;
    }
    for hit in &mut surface.hits {
        hit.rect.x += x;
    }
}
pub fn surfaces(l: Layout, t: &Appearance, m: &Model<'_>) -> (Surface, Surface) {
    let body = Layout::body(
        l.width.saturating_sub(l.sidebar.width).max(1),
        l.height,
        m.space,
    );
    let (mut chrome, mut foreground) = body_surfaces(body, t, m);
    move_surface(&mut chrome, l.sidebar.width, l.width);
    move_surface(&mut foreground, l.sidebar.width, l.width);
    let mut p = Pen { surface: chrome };
    let w = l.sidebar.width;
    p.fill(R::new(0, 0, w, l.height), &t.bg);
    p.grid(R::new(0, 48, w, l.height.saturating_sub(48)), &t.grid);
    p.fill(R::new(w.saturating_sub(1), 0, 1, l.height), &t.border);
    p.fill(R::new(0, 0, w.saturating_sub(1), 48), &t.panel);
    p.text(
        R::new(16, 14, w.saturating_sub(24), 25),
        "S T A R / M U X",
        &t.fg,
        if w >= 188 { 12 } else { 9 },
        true,
    );
    p.text(
        R::new(16, 63, w.saturating_sub(60), 22),
        "PROJECTS",
        &t.muted,
        11,
        true,
    );
    p.text(
        R::new(w.saturating_sub(40), 63, 28, 22),
        m.project_total.to_string(),
        &t.accent,
        11,
        true,
    );
    let mode = R::new(8, 86, w.saturating_sub(108).min(116), 24);
    if m.sort_mode_hover {
        p.border(mode, &t.border);
    }
    p.centered_text(
        mode,
        if m.project_manual {
            "Manual ↕"
        } else {
            "Activity"
        },
        &t.muted,
        11,
        false,
    );
    p.hit(mode, "projects:sort:mode");
    if m.project_manual {
        if w >= 220 {
            p.text(
                R::new(w - 120, 92, 108, 18),
                "Drag to reorder",
                &t.muted,
                9,
                false,
            );
        }
    } else {
        let sort = R::new(w.saturating_sub(84), 86, 68, 24);
        if m.sort_hover {
            p.border(sort, &t.border);
        }
        p.centered_text(
            sort,
            if m.project_sort_high {
                "High ↓"
            } else {
                "Low ↑"
            },
            &t.muted,
            11,
            false,
        );
        p.hit(sort, "projects:sort:direction");
    }
    let rows = project_rows(l.height);
    let start = m.project_offset.min(m.projects.len().saturating_sub(rows));
    for (n, project) in m.projects.iter().enumerate().skip(start).take(rows) {
        let y = 122 + (n - start) as u16 * 34;
        let r = R::new(8, y, w.saturating_sub(20), 32);
        let selected = project.id == m.project;
        if m.project_drag.is_some_and(|(id, _)| id == project.id) {
            p.border(r, &t.border);
        }
        if selected {
            p.fill(R::new(r.x, r.y, 2, r.height), &t.accent);
        }
        p.text(
            R::new(17, y + 5, 22, 18),
            format!("{:02}", n + 1),
            if selected { &t.accent } else { &t.muted },
            9,
            false,
        );
        let label = R::new(43, y + 2, w.saturating_sub(83), 19);
        let working = project.working > 0;
        if project.waiting > 0 {
            let r = R::new(w.saturating_sub(33), y + 5, 14, 14);
            p.text(
                R::new(r.x + 3, r.y.saturating_sub(2), 10, 19),
                if project.questions > 0 { "?" } else { "!" },
                &t.warning,
                16,
                true,
            );
        } else if working {
            activity_icon(
                &mut p,
                [w.saturating_sub(34), y + 4],
                t,
                ((if working || project.waiting > 0 {
                    m.wave_phase
                } else {
                    m.wave_phase / 2
                }) + n as u16 * 7)
                    % 48,
                m.reduced_motion,
                project.waiting > 0,
                working,
            );
        }
        let base = if selected || working {
            &t.accent
        } else {
            &t.fg
        };
        if project.name.is_ascii() {
            // Fixed mono positions keep names still as the activity wave passes.
            // A broad crest travels back and forth across the actual name;
            // short names no longer spend most of a cycle outside the wave.
            let phase = f64::from((m.wave_phase + n as u16 * 7) % 48) / 48.;
            let center = (1. - (phase * std::f64::consts::TAU).cos())
                * 0.5
                * project.name.len().saturating_sub(1) as f64;
            for (i, c) in project.name.chars().enumerate() {
                let offset = (i as f64 * 7.2).round() as u16;
                if offset >= label.width {
                    break;
                }
                let strength = if project.working > 0 {
                    if m.reduced_motion {
                        0.65
                    } else {
                        let distance = ((i as f64 - center).abs() / 6.).min(1.);
                        0.12 + 0.88 * ((distance * std::f64::consts::PI).cos() + 1.) * 0.5
                    }
                } else {
                    0.
                };
                let color = t.activity_color(base, strength);
                p.text(
                    R::new(
                        label.x + offset,
                        label.y,
                        10.min(label.width - offset),
                        label.height,
                    ),
                    c.to_string(),
                    &color,
                    12,
                    selected || working,
                );
            }
        } else {
            let strength = if project.working > 0 {
                if m.reduced_motion {
                    0.65
                } else {
                    ((f64::from(m.wave_phase) / 48. * std::f64::consts::TAU).sin() + 1.) * 0.5
                }
            } else {
                0.
            };
            p.text(
                label,
                project.name,
                &t.activity_color(base, strength),
                12,
                selected || working,
            );
        }
        let detail = if working && project.waiting > 0 {
            format!("{} working · {} waiting", project.working, project.waiting)
        } else if project.working > 0 {
            format!("{} · {} working", project.host, project.working)
        } else if project.questions > 0 {
            format!("{} · question pending", project.host)
        } else if project.waiting > 0 {
            format!("{} · approval requested", project.host)
        } else {
            if project.running > 0 {
                format!(
                    "{} · {} agent{}",
                    project.host,
                    project.running,
                    if project.running == 1 { "" } else { "s" }
                )
            } else {
                project.host.into()
            }
        };
        p.text(
            R::new(43, y + 20, w.saturating_sub(61), 12),
            detail,
            if project.waiting > 0 {
                &t.warning
            } else if working {
                &t.accent
            } else {
                &t.muted
            },
            9,
            false,
        );
        // The entire row is the selection target, including its padding and
        // activity mark. More specific controls are layered after this target.
        p.hit(R::new(0, y, w, 34), format!("project:{}", project.id));
    }
    if let Some((_, Some(gap))) = m.project_drag
        && gap >= start
        && gap <= start + rows
    {
        p.fill(
            R::new(12, 122 + (gap - start) as u16 * 34, w.saturating_sub(28), 2),
            &t.accent,
        );
    }
    if m.projects.len() > rows {
        let track = (rows as u16 * 34).min(l.height.saturating_sub(198));
        let thumb = (usize::from(track) * rows / m.projects.len()).max(18) as u16;
        let offset = (usize::from(track.saturating_sub(thumb)) * start
            / m.projects.len().saturating_sub(rows).max(1)) as u16;
        p.fill(R::new(w.saturating_sub(6), 122, 2, track), &t.border);
        p.fill(
            R::new(w.saturating_sub(6), 122 + offset, 2, thumb),
            &t.accent,
        );
    }
    if l.height > 140 {
        p.button(
            R::new(14, l.height - 66, w.saturating_sub(28), 30),
            "+ Add project",
            "project:new",
            t,
            false,
        );
        p.text(
            R::new(14, l.height - 28, w.saturating_sub(24), 21),
            if m.projects.len() > rows {
                format!(
                    "{}–{} of {} · scroll",
                    start + 1,
                    (start + rows).min(m.projects.len()),
                    m.projects.len()
                )
            } else {
                format!("{} projects · all visible", m.projects.len())
            },
            &t.muted,
            9,
            false,
        );
    }
    let mut fg = Pen {
        surface: foreground,
    };
    if m.git_open {
        draw_git(&mut fg, l, t, m);
    }
    if let Some(menu) = m.context_menu {
        draw_context_menu(&mut fg, l, t, menu);
    }
    (p.surface, fg.surface)
}

fn body_surfaces(l: Layout, t: &Appearance, m: &Model<'_>) -> (Surface, Surface) {
    let space = if !m.app_view && m.space == Space::Apps {
        Space::Terminals
    } else {
        m.space
    };
    let mut p = Pen::new(l.width, l.height, &t.bg);
    let mut fg = Pen::new(l.width, l.height, "transparent");
    p.grid(R::new(0, 84, l.width, l.height.saturating_sub(84)), &t.grid);
    p.fill(R::new(0, 0, l.width, 82), &t.panel);
    p.fill(R::new(0, 81, l.width, 1), &t.border);
    let theme_w = if l.width > 850 { 150 } else { 100 };
    let refresh_w = if l.width > 720 { 104 } else { 0 };
    p.text(
        R::new(
            20,
            10,
            l.width.saturating_sub(theme_w + 205 + refresh_w),
            26,
        ),
        m.project_label,
        &t.fg,
        18,
        true,
    );
    p.hit(
        R::new(16, 8, l.width.saturating_sub(theme_w + 185 + refresh_w), 28),
        format!("project:{}", m.project),
    );
    p.text(
        R::new(20, 37, l.width.saturating_sub(210), 17),
        m.directory,
        &t.muted,
        11,
        false,
    );
    if l.width > 600 {
        let x = l.width.saturating_sub(theme_w + 14);
        p.fill(R::new(x + 9, 43, 4, 4), &t.accent);
        p.text(
            R::new(x + 21, 37, theme_w.saturating_sub(30), 17),
            m.host,
            &t.muted,
            11,
            false,
        );
    }
    p.button(
        R::new(l.width.saturating_sub(theme_w + 14), 10, theme_w, 26),
        m.theme,
        "theme:next",
        t,
        false,
    );
    if m.app_view && l.width > 600 {
        p.button(
            R::new(l.width.saturating_sub(theme_w + 144), 10, 122, 26),
            m.audio_label,
            "audio:toggle",
            t,
            m.audio_label == "Muted",
        );
    }
    if refresh_w > 0 {
        p.button(
            R::new(l.width.saturating_sub(theme_w + 248), 10, 96, 26),
            "Refresh",
            "ui:refresh",
            t,
            false,
        );
    }
    let git_summary = m
        .git
        .map_or_else(|| "Checking Git…".into(), |g| g.summary());
    p.text(
        R::new(20, 59, l.width.saturating_sub(40), 20),
        git_summary,
        if m.git
            .is_some_and(|g| g.conflicts > 0 || !g.operation.is_empty())
        {
            &t.warning
        } else if m.git.is_some_and(|g| g.kind == "repo") {
            &t.accent
        } else {
            &t.muted
        },
        11,
        false,
    );
    p.hit(
        R::new(16, 56, l.width.saturating_sub(32), 24),
        "git:details",
    );
    p.fill(l.frame, &t.bg);
    p.fill(R::new(l.frame.x, l.frame.y, l.frame.width, 37), &t.panel);
    p.border(l.frame, if m.hover { &t.warning } else { &t.accent });
    let title = match space {
        Space::Agent | Space::Terminals => {
            if m.agent_state == "stopped" {
                "Terminal exited · right-click its tab to reopen a shell".into()
            } else if m.agent_name.is_empty() {
                format!("Project terminals / {}", m.host)
            } else {
                format!("{} · {} / {}", m.agent_name, m.agent_state, m.host)
            }
        }
        Space::Apps => format!("Virtual app desktop / {}", m.desktop_host),
    };
    if space != Space::Apps {
        p.text(
            R::new(
                l.frame.x + 12,
                l.frame.y + 9,
                l.frame
                    .width
                    .saturating_sub(if m.app_view { 430 } else { 310 }),
                24,
            ),
            title,
            &t.fg,
            13,
            false,
        );
    }
    if space == Space::Apps {
        let y = l.frame.y + 6;
        p.button(
            R::new(
                l.frame.x + 12,
                y,
                136.min(l.frame.width.saturating_sub(24)),
                26,
            ),
            "‹ Terminals",
            "view:terminals",
            t,
            false,
        );
        if l.frame.width >= 470 {
            let x = l.width.saturating_sub(310);
            p.button(R::new(x, y, 54, 26), "Tile", "apps:tile", t, false);
            p.button(
                R::new(x + 60, y, 82, 26),
                "Maximize",
                "apps:maximize",
                t,
                false,
            );
            p.button(R::new(x + 148, y, 62, 26), "Close", "apps:close", t, false);
            p.button(
                R::new(x + 216, y, 78, 26),
                "Reconnect",
                "apps:reconnect",
                t,
                false,
            );
        }
    }
    if m.app_view && space != Space::Apps && l.frame.width >= 420 {
        p.button(
            R::new(l.width.saturating_sub(306), l.frame.y + 6, 106, 26),
            "App Space",
            "view:apps",
            t,
            false,
        );
    }
    if space != Space::Apps {
        let x = l.width.saturating_sub(190);
        let y = l.frame.y + 6;
        p.button(
            R::new(x, y, 34, 26),
            "−",
            "terminal:font:decrease",
            t,
            false,
        );
        p.button(
            R::new(x + 40, y, 66, 26),
            &m.terminal_font_size,
            "terminal:font:reset",
            t,
            false,
        );
        p.button(
            R::new(x + 112, y, 34, 26),
            "+",
            "terminal:font:increase",
            t,
            false,
        );
    }
    if space != Space::Apps {
        let tab_left = l.tabs.x;
        let mut x = tab_left + TAB_LEAD;
        let mut controls = if m.split { 275 } else { 175 };
        // Shape the marker with the label so their complete visual group is
        // centered. An independently anchored dot made the right padding uneven.
        let labels = m
            .terminals
            .iter()
            .map(
                |(id, name)| match m.terminal_status.get(id).map(String::as_str) {
                    Some(
                        "needs_input"
                        | "waiting_for_approval"
                        | "needs_approval"
                        | "needs_permission",
                    ) => format!("{name} ?"),
                    Some("stopped" | "disconnected") => format!("{name} ×"),
                    Some(_) => format!("{name} ▪"),
                    None => name.clone(),
                },
            )
            .collect::<Vec<_>>();
        let widths: Vec<_> = labels
            .iter()
            .map(|label| (label.chars().count() as u16 * 8 + 34).clamp(90, 240))
            .collect();
        let available = l.width.saturating_sub(controls + x + TAB_LEAD + 12);
        let overflow = widths.iter().map(|w| u32::from(*w)).sum::<u32>() > u32::from(available);
        if overflow {
            controls += 68;
        }
        let available = l.width.saturating_sub(controls + x + TAB_LEAD + 12);
        let selected = m
            .terminals
            .iter()
            .position(|(id, _)| *id == m.terminal)
            .unwrap_or(0);
        let mut start = 0;
        while start < selected
            && widths[start..=selected]
                .iter()
                .map(|w| u32::from(*w))
                .sum::<u32>()
                > u32::from(available)
        {
            start += 1;
        }
        // Give the baseline the same short lead and tail, with an explicit
        // gap before the fixed controls rather than filling the whole strip.
        let tabs_right = l.width.saturating_sub(controls + TAB_LEAD + 12);
        let mut tab_end = x;
        for width in widths.iter().skip(start) {
            if tab_end + width > tabs_right {
                break;
            }
            tab_end += width;
        }
        let baseline_end = (tab_end + TAB_LEAD).min(l.width.saturating_sub(controls + 12));
        p.fill(
            R::new(
                tab_left,
                l.tabs.y + l.tabs.height - 1,
                baseline_end.saturating_sub(tab_left),
                1,
            ),
            &t.tab_color(),
        );
        for (((id, _), label), width) in m.terminals.iter().zip(&labels).zip(widths).skip(start) {
            if x + width > tabs_right {
                break;
            }
            p.tab(
                R::new(
                    x,
                    l.tabs.y + l.tabs.height.saturating_sub(TAB_HEIGHT),
                    width,
                    TAB_HEIGHT.min(l.tabs.height),
                ),
                label,
                format!("terminal:{id}"),
                t,
                *id == m.terminal,
            );
            x += width
        }
        if overflow {
            p.button(
                R::new(l.width.saturating_sub(controls), l.tabs.y + 3, 28, 27),
                "‹",
                "terminal:previous",
                t,
                false,
            );
            p.button(
                R::new(l.width.saturating_sub(controls) + 34, l.tabs.y + 3, 28, 27),
                "›",
                "terminal:next",
                t,
                false,
            );
        }
        if m.split {
            p.button(
                R::new(l.width.saturating_sub(275), l.tabs.y + 3, 94, 27),
                "Close pane",
                "terminal:close-pane",
                t,
                false,
            );
        }
        p.button(
            R::new(l.width.saturating_sub(175), l.tabs.y + 3, 74, 27),
            "+ Tab",
            "terminal:new",
            t,
            false,
        );
        p.button(
            R::new(l.width.saturating_sub(95), l.tabs.y + 3, 68, 27),
            if m.split { "Unsplit" } else { "Split" },
            "terminal:split",
            t,
            false,
        );
    }
    if space != Space::Apps && m.terminals.is_empty() {
        p.centered_text(
            R::new(
                l.canvas.x + 12,
                l.canvas.y + 30,
                l.canvas.width.saturating_sub(24),
                30,
            ),
            "No terminals open · use + Tab to start a shell",
            &t.muted,
            13,
            false,
        );
    }
    if space == Space::Apps && !m.connected {
        p.text(
            R::new(
                l.canvas.x + 24,
                l.canvas.y + 35,
                l.canvas.width.saturating_sub(48),
                45,
            ),
            "Launch an app, or reconnect to its existing desktop.",
            &t.muted,
            15,
            false,
        );
        p.text(R::new(l.canvas.x+24,l.canvas.y+88,l.canvas.width.saturating_sub(48),60),"The app host needs S T A R / M U X and a usable graphics device.\nAgent and terminal sessions stay available.",&t.muted,12,false);
    }
    if m.app_view && space != Space::Apps && l.shelf.height > 0 {
        let card = R::new(l.shelf.x, l.shelf.y, 190.min(l.shelf.width), l.shelf.height);
        p.fill(card, &t.panel);
        p.border(card, if m.connected { &t.accent } else { &t.border });
        desktop_map(
            &mut p,
            R::new(
                card.x + 10,
                card.y + 6,
                card.width.saturating_sub(20),
                card.height.saturating_sub(46),
            ),
            m.app_desktop,
            t,
        );
        p.centered_text(
            R::new(
                card.x + 2,
                card.y + card.height.saturating_sub(36),
                card.width.saturating_sub(4),
                19,
            ),
            "Open app space >",
            &t.fg,
            12,
            false,
        );
        let count = m.app_desktop.map_or(0, |d| d.windows.len());
        p.centered_text(
            R::new(
                card.x + 2,
                card.y + card.height.saturating_sub(18),
                card.width.saturating_sub(4),
                17,
            ),
            format!(
                "{count} {} · {}",
                if count == 1 { "app" } else { "apps" },
                m.desktop_host
            ),
            &t.muted,
            10,
            false,
        );
        p.hit(card, "view:apps");
    }
    if space == Space::Apps {
        p.fill(
            R::new(
                14,
                l.height.saturating_sub(36),
                l.width.saturating_sub(28),
                1,
            ),
            &t.tab_color(),
        );
        p.button(
            R::new(
                l.width.saturating_sub(160),
                l.height.saturating_sub(66),
                146,
                30,
            ),
            "+ Launch app",
            "launch-app",
            t,
            false,
        );
        let mut x = 22;
        for app in &m.apps {
            if x + 170 > l.width.saturating_sub(174) {
                break;
            }
            p.tab(
                R::new(x, l.height.saturating_sub(62), 160, 27),
                app.name,
                app.action.clone(),
                t,
                app.selected,
            );
            x += 160;
        }
    }
    p.fill(l.footer, &t.panel);
    p.fill(R::new(0, l.footer.y, l.width, 1), &t.border);
    p.text(
        R::new(
            14,
            l.footer.y + 5,
            l.width
                .saturating_sub(if m.transferring { 120 } else { 190 }),
            22,
        ),
        m.status,
        &t.muted,
        11,
        false,
    );
    if m.transferring {
        p.button(
            R::new(l.width.saturating_sub(110), l.footer.y + 2, 96, 24),
            "Cancel",
            "cancel-transfer",
            t,
            false,
        );
    } else {
        p.text(
            R::new(l.width.saturating_sub(185), l.footer.y + 5, 170, 20),
            "Ctrl+Esc / release",
            &t.muted,
            11,
            false,
        );
    }
    if let Some(modal) = &m.modal {
        draw_modal(&mut fg, l, t, modal)
    }
    (p.surface, fg.surface)
}
pub fn git_rows(height: u16) -> usize {
    usize::from(680.min(height.saturating_sub(24)).saturating_sub(282) / 26)
}
fn draw_git(p: &mut Pen, l: Layout, t: &Appearance, m: &Model<'_>) {
    let w = 1000.min(l.width.saturating_sub(24));
    let h = 680.min(l.height.saturating_sub(24));
    let r = R::new((l.width - w) / 2, (l.height - h) / 2, w, h);
    p.hit(R::new(0, 0, l.width, l.height), "git:close");
    p.fill(R::new(r.x + 4, r.y + 5, w, h), &t.bg);
    p.fill(r, &t.panel);
    p.border(r, &t.border);
    p.hit(r, "git:panel");
    let mut body = Pen::new(w, h, "transparent");
    body.text(
        R::new(20, 14, w.saturating_sub(180), 28),
        "Working tree",
        &t.fg,
        18,
        true,
    );
    body.button(
        R::new(w.saturating_sub(144), 14, 90, 27),
        "Refresh",
        "git:refresh",
        t,
        false,
    );
    body.button(
        R::new(w.saturating_sub(44), 14, 26, 27),
        "x",
        "git:close",
        t,
        false,
    );
    if let Some(g) = m.git {
        if g.kind == "repo" {
            body.text(
                R::new(20, 50, w.saturating_sub(40), 23),
                format!(
                    "{}{}  ↑{} ↓{}  {}",
                    g.branch,
                    if g.unborn { " / no commits" } else { "" },
                    g.ahead,
                    g.behind,
                    g.operation
                ),
                &t.accent,
                14,
                true,
            );
            body.text(
                R::new(20, 77, w.saturating_sub(40), 19),
                format!(
                    "{}  ·  {}",
                    g.root,
                    if g.upstream.is_empty() {
                        "No upstream configured"
                    } else {
                        &g.upstream
                    }
                ),
                &t.muted,
                11,
                false,
            );
            let col = w.saturating_sub(40) / 3;
            for (i, title, value, detail, color) in [
                (
                    0,
                    "STAGED",
                    g.staged,
                    format!(
                        "+{} −{} / ready to commit",
                        g.staged_lines.additions, g.staged_lines.deletions
                    ),
                    &t.success,
                ),
                (
                    1,
                    "UNSTAGED",
                    g.unstaged,
                    format!(
                        "+{} −{} / working changes",
                        g.unstaged_lines.additions, g.unstaged_lines.deletions
                    ),
                    &t.warning,
                ),
                (
                    2,
                    "UNTRACKED",
                    g.untracked,
                    format!("{} conflicts · {} stashes", g.conflicts, g.stash),
                    if g.conflicts > 0 { &t.error } else { &t.accent },
                ),
            ] {
                let x = 20 + i * col;
                body.fill(R::new(x, 108, col.saturating_sub(10), 66), &t.bg);
                body.text(
                    R::new(x + 10, 114, col.saturating_sub(30), 17),
                    title,
                    &t.muted,
                    10,
                    true,
                );
                body.text(
                    R::new(x + 10, 133, 48, 30),
                    value.to_string(),
                    color,
                    22,
                    true,
                );
                body.text(
                    R::new(x + 59, 139, col.saturating_sub(75), 25),
                    detail,
                    &t.muted,
                    10,
                    false,
                );
            }
            body.text(
                R::new(20, 185, w.saturating_sub(40), 20),
                if g.last_commit.is_empty() {
                    "No commits yet"
                } else {
                    &g.last_commit
                },
                &t.muted,
                11,
                false,
            );
            body.fill(R::new(20, 211, w.saturating_sub(40), 1), &t.border);
            body.text(R::new(26, 217, 52, 20), "INDEX", &t.muted, 9, true);
            body.text(R::new(90, 217, 56, 20), "WORK", &t.muted, 9, true);
            body.text(
                R::new(157, 217, w.saturating_sub(180), 20),
                "CHANGED FILES",
                &t.muted,
                9,
                true,
            );
            let rows = git_rows(l.height);
            let start = m.git_scroll.min(g.files.len().saturating_sub(rows));
            for (i, file) in g.files.iter().skip(start).take(rows).enumerate() {
                let y = 242 + i as u16 * 26;
                if i % 2 == 0 {
                    body.fill(R::new(20, y, w.saturating_sub(40), 25), &t.bg);
                }
                let color = if file.conflict { &t.error } else { &t.fg };
                body.text(
                    R::new(32, y + 4, 38, 20),
                    &file.index,
                    if file.index == "." {
                        &t.muted
                    } else {
                        &t.success
                    },
                    12,
                    true,
                );
                body.text(
                    R::new(100, y + 4, 38, 20),
                    &file.worktree,
                    if file.conflict { &t.error } else { &t.warning },
                    12,
                    true,
                );
                body.text(
                    R::new(157, y + 4, w.saturating_sub(180), 20),
                    file.original
                        .as_ref()
                        .map_or_else(|| file.path.clone(), |old| format!("{old} → {}", file.path)),
                    color,
                    12,
                    false,
                );
            }
            if g.clean() {
                body.text(
                    R::new(28, 254, w.saturating_sub(56), 30),
                    "Working tree clean / all work committed",
                    &t.success,
                    14,
                    false,
                );
            }
            body.text(
                R::new(20, h.saturating_sub(31), w.saturating_sub(40), 20),
                format!(
                    "{} files · {}–{} shown{} · tracked line totals · scroll / ↑ ↓ · Esc / close",
                    g.total_files,
                    if g.files.is_empty() { 0 } else { start + 1 },
                    (start + rows).min(g.files.len()),
                    if g.truncated { " (first 1000)" } else { "" }
                ),
                &t.muted,
                10,
                false,
            );
        } else {
            body.text(
                R::new(20, 64, w.saturating_sub(40), h.saturating_sub(100)),
                if g.kind == "none" {
                    "This project directory is outside a Git repository."
                } else {
                    &g.error
                },
                &t.muted,
                13,
                false,
            );
        }
    } else {
        body.text(
            R::new(20, 64, w.saturating_sub(40), 30),
            "Reading repository state on the project host…",
            &t.muted,
            13,
            false,
        );
    }
    move_surface(&mut body.surface, r.x, l.width);
    for node in &mut body.surface.nodes {
        let rect = match node {
            Primitive::Fill { rect, .. }
            | Primitive::Text { rect, .. }
            | Primitive::Border { rect, .. } => rect,
            _ => continue,
        };
        rect.y += r.y;
    }
    for hit in &mut body.surface.hits {
        hit.rect.y += r.y;
    }
    let offset = p.surface.nodes.len() as u16;
    p.surface
        .centered_text
        .extend(body.surface.centered_text.into_iter().map(|i| i + offset));
    p.surface.nodes.extend(body.surface.nodes);
    p.surface.hits.extend(body.surface.hits);
}
fn draw_context_menu(p: &mut Pen, l: Layout, t: &Appearance, m: &ContextMenu) {
    let r = m.bounds(l.width, l.height);
    // Capture the full viewport so dismissal never clicks through into a session.
    p.hit(R::new(0, 0, l.width, l.height), "menu:dismiss");
    p.fill(R::new(r.x + 4, r.y + 5, r.width, r.height), &t.bg);
    p.fill(r, &t.panel);
    p.border(r, &t.border);
    p.hit(r, "menu:panel");
    p.text(
        R::new(r.x + 16, r.y + 12, r.width.saturating_sub(32), 18),
        &m.title,
        &t.fg,
        12,
        true,
    );
    p.text(
        R::new(r.x + 16, r.y + 32, r.width.saturating_sub(32), 16),
        &m.subtitle,
        &t.muted,
        10,
        false,
    );
    p.fill(
        R::new(r.x + 12, r.y + 56, r.width.saturating_sub(24), 1),
        &t.border,
    );
    for (i, item) in m.items.iter().enumerate() {
        let y = r.y + 63 + i as u16 * 34;
        if y + 32 > r.y + r.height {
            break;
        }
        let row = R::new(r.x + 6, y, r.width.saturating_sub(12), 32);
        if m.selected == Some(i) {
            p.fill(row, &t.selection);
            p.fill(R::new(row.x, row.y + 6, 2, 20), &t.accent);
        }
        p.text(
            R::new(row.x + 12, y + 7, row.width.saturating_sub(70), 22),
            &item.label,
            if m.selected == Some(i) {
                &t.accent
            } else {
                &t.fg
            },
            12,
            false,
        );
        p.text(
            R::new(row.x + row.width.saturating_sub(54), y + 8, 44, 20),
            &item.hint,
            &t.muted,
            10,
            false,
        );
        p.hit(row, format!("menu:item:{i}"));
    }
}
pub fn application_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(190).min(410) / 42).max(1)
}
fn draw_application_picker(
    p: &mut Pen,
    l: Layout,
    t: &Appearance,
    m: &Modal<'_>,
    apps: &ApplicationPicker<'_>,
) {
    let w = 940.min(l.width.saturating_sub(24));
    let h = 600.min(l.height.saturating_sub(20));
    let r = R::new((l.width - w) / 2, (l.height - h) / 2, w, h);
    p.fill(r, &t.bg);
    p.border(r, &t.accent);
    p.text(
        R::new(r.x + 20, r.y + 14, w.saturating_sub(70), 30),
        "Launch application",
        &t.fg,
        18,
        true,
    );
    p.button(
        R::new(r.x + w - 40, r.y + 11, 26, 26),
        "x",
        "modal:cancel",
        t,
        false,
    );
    let split = w >= 676;
    let left = if split {
        w * 45 / 100
    } else {
        w.saturating_sub(40)
    };
    let search = R::new(r.x + 20, r.y + 62, left, 34);
    p.fill(search, &t.panel);
    p.border(search, if apps.focused { &t.accent } else { &t.border });
    let text = if apps.focused {
        format!(
            "{}|{}",
            &apps.query[..apps.cursor],
            &apps.query[apps.cursor..]
        )
    } else if apps.query.is_empty() {
        "Search applications…".into()
    } else {
        apps.query.into()
    };
    p.text(
        R::new(
            search.x + 9,
            search.y + 6,
            search.width.saturating_sub(18),
            24,
        ),
        text,
        &t.fg,
        13,
        false,
    );
    p.hit(search, "modal:search");
    p.text(
        R::new(search.x, r.y + 104, left, 20),
        format!("{} / {} matches", apps.status, apps.count),
        &t.muted,
        10,
        false,
    );
    for (row, (index, name, executable)) in apps.rows.iter().enumerate() {
        let y = r.y + 132 + row as u16 * 42;
        if y + 40 > r.y + h.saturating_sub(56) {
            break;
        }
        let rect = R::new(search.x, y, left, 40);
        if *index == apps.selected {
            p.fill(rect, &t.selection);
            p.fill(R::new(rect.x, rect.y + 5, 2, 30), &t.accent);
        }
        p.text(
            R::new(rect.x + 10, rect.y + 3, left.saturating_sub(20), 21),
            *name,
            if *index == apps.selected {
                &t.accent
            } else {
                &t.fg
            },
            13,
            false,
        );
        p.text(
            R::new(rect.x + 10, rect.y + 23, left.saturating_sub(20), 16),
            *executable,
            &t.muted,
            10,
            false,
        );
        p.hit(rect, format!("modal:application:{index}"));
    }
    if apps.rows.is_empty() {
        p.text(
            R::new(search.x + 10, r.y + 143, left.saturating_sub(20), 60),
            if apps.query.is_empty() {
                "No applications loaded"
            } else {
                "No matching applications"
            },
            &t.muted,
            12,
            false,
        );
    }
    if split {
        let x = r.x + left + 42;
        let width = w.saturating_sub(left + 62);
        for (i, f) in m.fields.iter().enumerate() {
            let y = r.y + 62 + i as u16 * 84;
            if y + 60 > r.y + h.saturating_sub(70) {
                break;
            }
            p.text(
                R::new(x, y, width, 21),
                if i == 0 && apps.selection_name.is_some() {
                    "Application"
                } else {
                    f.label
                },
                &t.muted,
                11,
                false,
            );
            let field = R::new(x, y + 25, width, 34);
            p.fill(field, &t.panel);
            p.border(
                field,
                if !apps.focused && i == m.selected {
                    &t.accent
                } else {
                    &t.border
                },
            );
            let text = if let Some(name) = apps.selection_name.filter(|_| i == 0) {
                name.into()
            } else if !apps.focused && i == m.selected {
                format!("{}|{}", &f.text[..f.cursor], &f.text[f.cursor..])
            } else {
                f.text.into()
            };
            p.text(
                R::new(x + 9, y + 31, width.saturating_sub(18), 24),
                text,
                &t.fg,
                12,
                false,
            );
            p.hit(
                field,
                if i == 0 && apps.selection_name.is_some() {
                    "modal:details".into()
                } else {
                    format!("modal:field:{i}")
                },
            );
        }
        p.text(
            R::new(x, r.y + h.saturating_sub(96), width, 40),
            m.error,
            &t.error,
            11,
            false,
        );
    }
    if !m.error.is_empty() {
        p.text(
            R::new(
                r.x + 20,
                r.y + h.saturating_sub(70),
                w.saturating_sub(40),
                22,
            ),
            m.error,
            &t.error,
            11,
            false,
        );
    }
    p.button(
        R::new(r.x + 20, r.y + h.saturating_sub(43), 132, 27),
        "Custom / host…",
        "modal:details",
        t,
        false,
    );
    p.text(
        R::new(
            r.x + 164,
            r.y + h.saturating_sub(37),
            w.saturating_sub(320),
            22,
        ),
        "↑↓ / select · Enter / launch",
        &t.muted,
        10,
        false,
    );
    p.button(
        R::new(r.x + w - 120, r.y + h.saturating_sub(43), 100, 27),
        "Launch",
        "modal:accept",
        t,
        false,
    );
}
fn draw_modal(p: &mut Pen, l: Layout, t: &Appearance, m: &Modal<'_>) {
    if let Some(apps) = &m.applications {
        draw_application_picker(p, l, t, m, apps);
        return;
    }
    let w = 640.min(l.width.saturating_sub(24));
    let h = (135 + m.fields.len() as u16 * 68).min(l.height.saturating_sub(20));
    let r = R::new((l.width - w) / 2, (l.height - h) / 2, w, h);
    p.fill(r, &t.bg);
    p.border(r, &t.accent);
    p.text(
        R::new(r.x + 20, r.y + 14, w.saturating_sub(70), 31),
        m.title,
        &t.fg,
        18,
        true,
    );
    p.button(
        R::new(r.x + w - 40, r.y + 11, 26, 26),
        "x",
        "modal:cancel",
        t,
        false,
    );
    for (i, f) in m.fields.iter().enumerate() {
        let y = r.y + 57 + i as u16 * 68;
        if y + 58 > r.y + h - 42 {
            break;
        }
        p.text(
            R::new(r.x + 20, y, w.saturating_sub(40), 20),
            f.label,
            &t.muted,
            11,
            false,
        );
        let field = R::new(r.x + 20, y + 23, w.saturating_sub(40), 34);
        p.fill(field, &t.panel);
        p.border(
            field,
            if i == m.selected {
                &t.accent
            } else {
                &t.border
            },
        );
        let text = if i == m.selected {
            format!("{}|{}", &f.text[..f.cursor], &f.text[f.cursor..])
        } else {
            f.text.into()
        };
        p.text(
            R::new(field.x + 9, field.y + 6, field.width.saturating_sub(18), 25),
            text,
            &t.fg,
            13,
            false,
        );
        p.hit(field, format!("modal:field:{i}"));
    }
    p.text(
        R::new(r.x + 20, r.y + h - 56, w.saturating_sub(40), 23),
        m.error,
        &t.error,
        11,
        false,
    );
    if m.catalog_available {
        p.button(
            R::new(r.x + 20, r.y + h - 36, 152, 27),
            "Browse system apps",
            "modal:catalog",
            t,
            false,
        );
    } else {
        p.text(
            R::new(r.x + 20, r.y + h - 30, w.saturating_sub(165), 21),
            "Tab / next   Ctrl+Enter / accept",
            &t.muted,
            10,
            false,
        );
    }
    p.button(
        R::new(r.x + w - 120, r.y + h - 36, 100, 27),
        "Accept",
        "modal:accept",
        t,
        false,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_map_keeps_aspect_and_clips_offscreen_windows() {
        let bounds = desktop_bounds(R::new(10, 20, 140, 80), [1000, 1000]).unwrap();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (40, 20, 80, 80)
        );
        let mut window = crate::project::WindowLayout {
            app_id: "test".into(),
            x: -250,
            y: 500,
            width: 750,
            height: 1000,
        };
        let rect = map_window(bounds, [1000, 1000], &window).unwrap();
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (40, 60, 40, 40));
        window.x = 1000;
        assert!(map_window(bounds, [1000, 1000], &window).is_none());
        assert!(desktop_bounds(bounds, [0, 1000]).is_none());
    }
    #[test]
    fn native_surfaces_validate_in_all_spaces_and_sizes() {
        let theme = Appearance::load("kanagawa").unwrap();
        let windows = [
            crate::project::WindowLayout {
                app_id: "handy".into(),
                x: 0,
                y: 0,
                width: 1280,
                height: 1080,
            },
            crate::project::WindowLayout {
                app_id: "support".into(),
                x: 1280,
                y: 0,
                width: 640,
                height: 1080,
            },
        ];
        for (width, height) in [(320, 240), (800, 600), (1400, 900), (3424, 2150)] {
            for space in [Space::Agent, Space::Terminals, Space::Apps] {
                let mut model = Model {
                    app_view: false,
                    project: "sample",
                    project_label: "sample",
                    project_offset: 0,
                    project_total: 1,
                    sort_hover: false,
                    sort_mode_hover: false,
                    project_manual: true,
                    project_drag: None,
                    project_sort_high: true,
                    wave_phase: 0,
                    reduced_motion: false,
                    directory: "/work/sample",
                    host: "ssh-host",
                    desktop_host: "local",
                    theme: "Kanagawa",
                    audio_label: "Audio on",
                    space,
                    projects: vec![],
                    terminals: vec![(0, "Shell".into())],
                    terminal_status: Default::default(),
                    terminal: 0,
                    terminal_font_size: "16px".into(),
                    split: false,
                    agent_name: "Codex",
                    agent_state: "running",
                    status: "Ready",
                    apps: vec![
                        AppCard {
                            action: "app:desktop:1".into(),
                            name: "Handy",
                            selected: true,
                        },
                        AppCard {
                            action: "app:desktop:2".into(),
                            name: "Supporting app",
                            selected: false,
                        },
                    ],
                    app_desktop: Some(DesktopMap {
                        size: [1920, 1080],
                        windows: &windows,
                        selected: Some(0),
                    }),
                    connected: false,
                    git: None,
                    git_open: false,
                    git_scroll: 0,
                    context_menu: None,
                    modal: None,
                    transferring: false,
                    hover: false,
                };
                let (chrome, foreground) =
                    surfaces(Layout::new(width, height, space), &theme, &model);
                assert!(
                    !chrome
                        .hits
                        .iter()
                        .any(|h| crate::features::app_action(&h.action)),
                    "Unsupported frontends must hide app controls, including restored app space"
                );
                chrome.validate().unwrap();
                foreground.validate().unwrap();
                model.app_view = true;
                let layout = Layout::new(width, height, space);
                let (enabled, _) = surfaces(layout, &theme, &model);
                if space == Space::Apps {
                    assert!(enabled.hits.iter().any(|h| h.action == "launch-app"));
                    assert!(enabled.hits.iter().any(|h| h.action == "view:terminals"));
                } else if layout.frame.width >= 420 {
                    assert!(enabled.hits.iter().any(|h| h.action == "view:apps"));
                }
                enabled.validate().unwrap();
                model.app_view = false;
                let menu = ContextMenu {
                    position: [width - 2, height - 2],
                    title: "Project".into(),
                    subtitle: "PROJECT / SSH".into(),
                    selected: Some(0),
                    items: vec![MenuItem {
                        label: "Rename…".into(),
                        hint: String::new(),
                        action: "rename-project:sample".into(),
                    }],
                };
                model.context_menu = Some(&menu);
                let (_, overlay) = surfaces(Layout::new(width, height, space), &theme, &model);
                overlay.validate().unwrap();
                let bounds = menu.bounds(width, height);
                assert!(bounds.x + bounds.width <= width && bounds.y + bounds.height <= height);
                assert_eq!(overlay.hits.first().unwrap().action, "menu:dismiss");
                assert_eq!(overlay.hits.last().unwrap().action, "menu:item:0");
                model.context_menu = None;
                model.git_open = true;
                let git = crate::git::Status {
                    kind: "repo".into(),
                    branch: "main".into(),
                    ..Default::default()
                };
                model.git = Some(&git);
                let (_, overlay) = surfaces(Layout::new(width, height, space), &theme, &model);
                overlay.validate().unwrap();
                assert_eq!(overlay.hits.first().unwrap().action, "git:close");
            }
        }
    }
    #[test]
    fn twenty_projects_are_visible_without_paging_and_small_lists_scroll() {
        let theme = Appearance::load("kanagawa").unwrap();
        let ids = (0..20)
            .map(|i| format!("project-{i:02}"))
            .collect::<Vec<_>>();
        for (height, offset) in [(900, 0), (500, 5)] {
            let model = Model {
                app_view: false,
                project: &ids[18],
                project_label: "Zendocon decompile",
                project_offset: offset,
                project_total: 20,
                sort_hover: false,
                sort_mode_hover: false,
                project_manual: true,
                project_drag: None,
                project_sort_high: true,
                wave_phase: 0,
                reduced_motion: false,
                directory: "/work/zendocon",
                host: "local",
                theme: "Kanagawa",
                audio_label: "Audio on",
                space: Space::Agent,
                projects: ids
                    .iter()
                    .map(|id| ProjectTab {
                        id,
                        name: id,
                        host: "local",
                        working: 0,
                        waiting: 0,
                        running: 0,
                        questions: 0,
                    })
                    .collect(),
                terminals: vec![(0, "Shell".into())],
                terminal_status: Default::default(),
                terminal: 0,
                terminal_font_size: "14.7px".into(),
                split: false,
                agent_name: "Codex",
                agent_state: "working",
                status: "Ready",
                apps: vec![],
                app_desktop: None,
                connected: false,
                desktop_host: "local",
                git: None,
                git_open: false,
                git_scroll: 0,
                context_menu: None,
                modal: None,
                transferring: false,
                hover: false,
            };
            let l = Layout::new(1400, height, Space::Agent);
            let (chrome, fg) = surfaces(l, &theme, &model);
            chrome.validate().unwrap();
            fg.validate().unwrap();
            let hits = chrome
                .hits
                .iter()
                .filter(|h| h.action.starts_with("project:project-") && h.rect.x < l.sidebar.width)
                .collect::<Vec<_>>();
            assert_eq!(hits.len(), 20.min(project_rows(height)));
            assert_eq!(hits[0].action, format!("project:{}", ids[offset]));
            assert!(
                hits.iter().all(|h| h.rect.x == 0
                    && h.rect.width == l.sidebar.width
                    && h.rect.height == 34)
            );
            assert!(
                !chrome
                    .hits
                    .iter()
                    .any(|h| h.action.starts_with("project:page:"))
            );
        }
    }
    #[test]
    fn hidden_app_shelf_returns_space_to_terminal() {
        let l = Layout::new(1400, 900, Space::Agent);
        assert_eq!(l.sidebar.width, 252);
        assert_eq!(l.canvas.width, 1118);
        assert!(l.canvas.x > l.sidebar.width);
        assert_eq!(l.shelf.height, 0);
        assert_eq!(l.canvas.x, l.sidebar.width + 15);
        assert_eq!(l.canvas.x + l.canvas.width + 15, l.width);
        assert_eq!(l.canvas.y + l.canvas.height + 13, l.footer.y);
        let apps = Layout::new(1400, 900, Space::Apps);
        assert!(apps.canvas.height >= l.canvas.height);
        assert!(apps.canvas.y < l.canvas.y);
    }
}
