//! App Shelf: single pixel borders, drafting grid, project spaces and app cards.
use crate::{appearance::Appearance, project_state::Space};
use starkit::native_surface::{HitRegion, PixelRect as R, Primitive, Surface};
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
        let y = 130.min(height.saturating_sub(100));
        let shelf_h = if space == Space::Apps {
            0
        } else {
            if height < 550 { 72 } else { 116 }
        };
        let bottom = height.saturating_sub(40 + shelf_h);
        let frame = R::new(x, y, w, bottom.saturating_sub(y).max(1));
        let tab_h = if space != Space::Apps { 34 } else { 0 };
        let canvas = R::new(
            x + 1,
            y + 37 + tab_h,
            w.saturating_sub(2).max(1),
            frame
                .height
                .saturating_sub(38 + tab_h + if space == Space::Apps { 34 } else { 0 })
                .max(1),
        );
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
pub struct AgentTab {
    pub id: usize,
    pub name: String,
    pub state: String,
}
pub struct ProjectTab<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub host: &'a str,
    pub working: usize,
    pub waiting: usize,
}
pub struct AppCard<'a> {
    pub action: String,
    pub name: &'a str,
    pub host: &'a str,
    pub selected: bool,
}
pub struct ModalField<'a> {
    pub label: &'a str,
    pub text: &'a str,
    pub cursor: usize,
}
pub struct Modal<'a> {
    pub title: &'a str,
    pub fields: Vec<ModalField<'a>>,
    pub selected: usize,
    pub error: &'a str,
}
#[derive(Clone)]
pub struct MenuItem {
    pub label: String,
    pub hint: String,
    pub action: String,
}
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
pub struct Model<'a> {
    pub project: &'a str,
    pub project_label: &'a str,
    pub project_offset: usize,
    pub project_total: usize,
    pub wave_phase: u16,
    pub reduced_motion: bool,
    pub directory: &'a str,
    pub host: &'a str,
    pub theme: &'a str,
    pub space: Space,
    pub projects: Vec<ProjectTab<'a>>,
    pub terminals: Vec<(usize, String)>,
    pub terminal: usize,
    pub terminal_font_size: String,
    pub split: bool,
    pub agent: usize,
    pub agents: Vec<AgentTab>,
    pub agent_name: &'a str,
    pub agent_state: &'a str,
    pub status: &'a str,
    pub apps: Vec<AppCard<'a>>,
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
        self.text(
            R::new(
                r.x + 9,
                r.y + 4,
                r.width.saturating_sub(18),
                r.height.saturating_sub(5),
            ),
            text,
            if selected { &t.accent } else { &t.fg },
            12,
            false,
        );
        self.hit(r, action)
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
pub fn project_rows(height: u16) -> usize {
    usize::from(height.saturating_sub(170) / 34).max(1)
}
fn activity_icon(p: &mut Pen, x: u16, y: u16, t: &Appearance, phase: u16, reduced_motion: bool) {
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
            &t.activity_color(&t.border, strength),
        );
    }
    p.fill(R::new(x + 7, y + 5, 2, 6), &t.accent);
    p.fill(R::new(x + 5, y + 7, 6, 2), &t.accent);
    p.fill(
        R::new(x + 7, y + 7, 2, 2),
        &t.activity_color(&t.accent, 0.85),
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
    let rows = project_rows(l.height);
    let start = m.project_offset.min(m.projects.len().saturating_sub(rows));
    for (n, project) in m.projects.iter().enumerate().skip(start).take(rows) {
        let y = 94 + (n - start) as u16 * 34;
        let r = R::new(8, y, w.saturating_sub(20), 32);
        let selected = project.id == m.project;
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
        if working {
            activity_icon(
                &mut p,
                w.saturating_sub(34),
                y + 4,
                t,
                (m.wave_phase + n as u16 * 7) % 48,
                m.reduced_motion,
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
        let detail = if project.working > 0 {
            format!("{} · {} working", project.host, project.working)
        } else if project.waiting > 0 {
            format!("{} · needs input", project.host)
        } else {
            project.host.into()
        };
        p.text(
            R::new(43, y + 20, w.saturating_sub(61), 12),
            detail,
            if working {
                &t.accent
            } else if project.waiting > 0 {
                &t.warning
            } else {
                &t.muted
            },
            9,
            false,
        );
        p.hit(r, format!("project:{}", project.id));
    }
    if m.projects.len() > rows {
        let track = (rows as u16 * 34).min(l.height.saturating_sub(170));
        let thumb = (usize::from(track) * rows / m.projects.len()).max(18) as u16;
        let offset = (usize::from(track.saturating_sub(thumb)) * start
            / m.projects.len().saturating_sub(rows).max(1)) as u16;
        p.fill(R::new(w.saturating_sub(6), 94, 2, track), &t.border);
        p.fill(
            R::new(w.saturating_sub(6), 94 + offset, 2, thumb),
            &t.accent,
        );
    }
    if l.height > 140 {
        p.button(
            R::new(14, l.height - 66, w.saturating_sub(28), 30),
            "+ New project",
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
    let mut p = Pen::new(l.width, l.height, &t.bg);
    let mut fg = Pen::new(l.width, l.height, "transparent");
    p.grid(R::new(0, 84, l.width, l.height.saturating_sub(84)), &t.grid);
    p.fill(R::new(0, 0, l.width, 82), &t.panel);
    p.fill(R::new(0, 81, l.width, 1), &t.border);
    let theme_w = if l.width > 850 { 150 } else { 100 };
    p.text(
        R::new(20, 10, l.width.saturating_sub(theme_w + 60), 26),
        m.project_label,
        &t.fg,
        18,
        true,
    );
    p.hit(
        R::new(16, 8, l.width.saturating_sub(theme_w + 48), 28),
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
    let mut x = 14;
    for (space, name, id) in [
        (Space::Agent, "Agent", "agent"),
        (Space::Terminals, "Terminals", "terminals"),
        (Space::Apps, "Apps", "apps"),
    ] {
        let w = if space == Space::Terminals { 114 } else { 86 };
        p.button(
            R::new(x, 90, w, 29),
            name,
            format!("view:{id}"),
            t,
            m.space == space,
        );
        x += w + 6;
    }
    if l.width > 960 {
        p.text(
            R::new(330, 94, l.width.saturating_sub(640), 23),
            "All inside this project",
            &t.muted,
            11,
            false,
        );
    }
    if l.width > 460 {
        p.button(
            R::new(l.width - 140, 90, 126, 29),
            "Launch app +",
            "launch-app",
            t,
            false,
        );
    }
    if l.width > 620 {
        p.button(
            R::new(l.width - 250, 90, 100, 29),
            "Run app",
            "run-app",
            t,
            false,
        );
    }
    p.fill(l.frame, &t.bg);
    p.fill(R::new(l.frame.x, l.frame.y, l.frame.width, 37), &t.panel);
    p.border(l.frame, if m.hover { &t.warning } else { &t.accent });
    let title = match m.space {
        Space::Agent => format!("{} / {}", m.agent_name, m.agent_state),
        Space::Terminals => format!("Project terminals / {}", m.host),
        Space::Apps => format!("Virtual app desktop / {}", m.desktop_host),
    };
    p.text(
        R::new(
            l.frame.x + 12,
            l.frame.y + 9,
            l.frame.width.saturating_sub(310),
            24,
        ),
        title,
        &t.fg,
        13,
        false,
    );
    if m.space == Space::Apps {
        let y = l.frame.y + 6;
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
    } else if m.space == Space::Agent && l.width >= 480 {
        p.button(
            R::new(l.width.saturating_sub(135), l.frame.y + 6, 108, 26),
            "Attach agent",
            "agent:attach",
            t,
            false,
        );
    }
    if m.space != Space::Apps {
        let x = l
            .width
            .saturating_sub(if m.space == Space::Agent && l.width >= 480 {
                313
            } else {
                190
            });
        let y = l.frame.y + 6;
        p.button(
            R::new(x, y, 34, 26),
            "A−",
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
            "A+",
            "terminal:font:increase",
            t,
            false,
        );
    }
    if m.space == Space::Agent {
        let available = l.width.saturating_sub(220);
        let count = usize::from(available / 158).max(1);
        let selected = m.agents.iter().position(|a| a.id == m.agent).unwrap_or(0);
        let start = selected.saturating_sub(count - 1);
        let mut x = l.tabs.x + 5;
        for agent in m.agents.iter().skip(start).take(count) {
            let r = R::new(x, l.tabs.y + 3, 152.min(l.width.saturating_sub(x)), 27);
            p.button(
                r,
                &agent.name,
                format!("agent:{}", agent.id),
                t,
                agent.id == m.agent,
            );
            p.fill(
                R::new(r.x + r.width.saturating_sub(10), r.y + 11, 4, 4),
                match agent.state.as_str() {
                    "stopped" | "disconnected" => &t.error,
                    "waiting" | "needs_input" => &t.warning,
                    "starting" => &t.muted,
                    _ => &t.success,
                },
            );
            x += 158;
        }
        if m.agents.len() > count && l.width > 430 {
            p.button(
                R::new(l.width - 182, l.tabs.y + 3, 28, 27),
                "‹",
                "agent:previous",
                t,
                false,
            );
            p.button(
                R::new(l.width - 148, l.tabs.y + 3, 28, 27),
                "›",
                "agent:next",
                t,
                false,
            );
        }
        p.button(
            R::new(l.width.saturating_sub(112), l.tabs.y + 3, 92, 27),
            "+ Agent",
            "agent:new",
            t,
            false,
        );
    }
    if m.space == Space::Terminals {
        let mut x = l.tabs.x + 5;
        let controls = if m.split { 275 } else { 175 };
        for (id, name) in &m.terminals {
            let width = (name.chars().count() as u16 * 8 + 24).clamp(80, 200);
            if x + width > l.width.saturating_sub(controls) {
                break;
            }
            p.button(
                R::new(x, l.tabs.y + 3, width, 27),
                name,
                format!("terminal:{id}"),
                t,
                *id == m.terminal,
            );
            x += width + 6
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
    if m.space == Space::Apps && !m.connected {
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
    if m.space != Space::Apps && l.shelf.height > 0 {
        p.text(
            R::new(l.shelf.x, l.shelf.y + 8, 90, 30),
            "PROJECT\nAPPS",
            &t.muted,
            11,
            false,
        );
        let mut x = l.shelf.x + 96;
        let card_w = 160u16;
        for app in &m.apps {
            if x + card_w > l.width.saturating_sub(175) {
                break;
            }
            let card = R::new(x, l.shelf.y, card_w, l.shelf.height);
            p.fill(card, &t.panel);
            p.border(card, if app.selected { &t.accent } else { &t.border });
            let preview = R::new(
                x + 1,
                l.shelf.y + 1,
                card_w - 2,
                l.shelf.height.saturating_sub(39),
            );
            p.grid(preview, &t.grid);
            p.text(
                R::new(x + 13, l.shelf.y + 8, 135, 23),
                "[ app ]",
                &t.accent,
                12,
                false,
            );
            p.text(
                R::new(
                    x + 9,
                    l.shelf.y + l.shelf.height.saturating_sub(36),
                    card_w - 18,
                    19,
                ),
                app.name,
                &t.fg,
                12,
                false,
            );
            p.text(
                R::new(
                    x + 9,
                    l.shelf.y + l.shelf.height.saturating_sub(18),
                    card_w - 18,
                    17,
                ),
                app.host,
                &t.muted,
                10,
                false,
            );
            p.hit(card, app.action.clone());
            x += card_w + 10;
        }
        p.button(
            R::new(x.min(l.width.saturating_sub(175)), l.shelf.y + 14, 146, 30),
            "+ Launch app",
            "launch-app",
            t,
            false,
        );
        p.button(
            R::new(
                l.width.saturating_sub(168),
                l.shelf.y + l.shelf.height.saturating_sub(32),
                154,
                29,
            ),
            "Open app space >",
            "view:apps",
            t,
            false,
        );
    }
    if m.space == Space::Apps {
        let mut x = 14;
        for app in &m.apps {
            if x + 170 > l.width.saturating_sub(28) {
                break;
            }
            p.button(
                R::new(x, l.height.saturating_sub(62), 160, 27),
                app.name,
                app.action.clone(),
                t,
                app.selected,
            );
            x += 166;
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
    // A corner pixel keeps the otherwise transparent overlay nonempty in GPU mode.
    fg.fill(R::new(0, 0, 1, 1), &t.border);
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
fn draw_modal(p: &mut Pen, l: Layout, t: &Appearance, m: &Modal<'_>) {
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
    p.text(
        R::new(r.x + 20, r.y + h - 30, w.saturating_sub(165), 21),
        "Tab / next   Ctrl+Enter / accept",
        &t.muted,
        10,
        false,
    );
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
    fn native_surfaces_validate_in_all_spaces_and_sizes() {
        let theme = Appearance::load("kanagawa").unwrap();
        for (width, height) in [(320, 240), (800, 600), (1400, 900), (3424, 2150)] {
            for space in [Space::Agent, Space::Terminals, Space::Apps] {
                let mut model = Model {
                    project: "sample",
                    project_label: "sample",
                    project_offset: 0,
                    project_total: 1,
                    wave_phase: 0,
                    reduced_motion: false,
                    directory: "/work/sample",
                    host: "ssh-host",
                    desktop_host: "local",
                    theme: "Kanagawa",
                    space,
                    projects: vec![],
                    terminals: vec![(0, "Shell".into())],
                    terminal: 0,
                    terminal_font_size: "16px".into(),
                    split: false,
                    agent: 1,
                    agents: vec![],
                    agent_name: "Codex",
                    agent_state: "running",
                    status: "Ready",
                    apps: vec![],
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
                chrome.validate().unwrap();
                foreground.validate().unwrap();
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
                project: &ids[18],
                project_label: "Zendocon decompile",
                project_offset: offset,
                project_total: 20,
                wave_phase: 0,
                reduced_motion: false,
                directory: "/work/zendocon",
                host: "local",
                theme: "Kanagawa",
                space: Space::Agent,
                projects: ids
                    .iter()
                    .map(|id| ProjectTab {
                        id,
                        name: id,
                        host: "local",
                        working: 0,
                        waiting: 0,
                    })
                    .collect(),
                terminals: vec![(0, "Shell".into())],
                terminal: 0,
                terminal_font_size: "14.7px".into(),
                split: false,
                agent: 1,
                agents: vec![],
                agent_name: "Codex",
                agent_state: "working",
                status: "Ready",
                apps: vec![],
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
                hits.iter()
                    .all(|h| h.rect.x + h.rect.width < l.sidebar.width)
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
    fn shelf_leaves_one_full_width_terminal() {
        let l = Layout::new(1400, 900, Space::Agent);
        assert_eq!(l.sidebar.width, 252);
        assert_eq!(l.canvas.width, 1118);
        assert!(l.canvas.x > l.sidebar.width);
        assert!(l.shelf.height > 80);
        assert!(l.canvas.y + l.canvas.height < l.shelf.y);
        let apps = Layout::new(1400, 900, Space::Apps);
        assert!(apps.canvas.height > l.canvas.height);
    }
}
