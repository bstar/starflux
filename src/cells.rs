//! Cell layout and input adapter for the shared workspace controller.
use super::*;
use starkit::ratatui::{
    buffer::Buffer,
    layout::Rect as CellRect,
    style::{Color, Modifier, Style},
};

#[derive(Clone)]
pub(super) struct Control {
    pub action: String,
    rect: Rect,
}
pub(super) struct Ui {
    pub terminal_focus: bool,
    pub sidebar: bool,
    focus: usize,
    controls: Vec<Control>,
    tab_offset: usize,
    drag_project: Option<String>,
    pointer_pane: Option<usize>,
    hover: Option<[u16; 2]>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            terminal_focus: true,
            sidebar: false,
            focus: 0,
            controls: vec![],
            tab_offset: 0,
            drag_project: None,
            pointer_pane: None,
            hover: None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Layout {
    sidebar: Rect,
    canvas: Rect,
    small: bool,
}
impl Layout {
    fn new(v: Viewport, sidebar: bool) -> Self {
        let small = v.columns < 60 || v.rows < 16;
        let side = if !small && (v.columns >= 100 || sidebar) {
            25
        } else {
            0
        };
        Self {
            sidebar: Rect {
                x: 0,
                y: 4,
                width: side,
                height: v.rows.saturating_sub(6),
            },
            canvas: Rect {
                x: side,
                y: 4,
                width: v.columns.saturating_sub(side),
                height: v.rows.saturating_sub(6),
            },
            small,
        }
    }
    fn panes(self, ids: &[usize], selected: usize, ratio: f64) -> Vec<(usize, Rect)> {
        if self.small || ids.is_empty() {
            return vec![];
        }
        let c = self.canvas;
        if ids.len() == 2 && c.width >= 30 {
            let left = ((f64::from(c.width - 1) * ratio) as u16).clamp(10, c.width - 11);
            vec![
                (ids[0], Rect { width: left, ..c }),
                (
                    ids[1],
                    Rect {
                        x: c.x + left + 1,
                        width: c.width - left - 1,
                        ..c
                    },
                ),
            ]
        } else {
            vec![(
                if ids.contains(&selected) {
                    selected
                } else {
                    ids[0]
                },
                c,
            )]
        }
    }
}
fn color(s: &str) -> Color {
    u32::from_str_radix(s.trim_start_matches('#'), 16)
        .map(|n| Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
        .unwrap_or(Color::Reset)
}
fn put(b: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    if x < b.area.width && y < b.area.height {
        let safe: String = text.chars().filter(|c| !c.is_control()).collect();
        b.set_stringn(x, y, safe, usize::from(width.min(b.area.width - x)), style);
    }
}
fn pixels(v: Viewport, r: Rect) -> PixelRect {
    let x = u32::from(r.x) * v.width / u32::from(v.columns.max(1));
    let y = u32::from(r.y) * v.height / u32::from(v.rows.max(1));
    let right = u32::from(r.x + r.width) * v.width / u32::from(v.columns.max(1));
    let bottom = u32::from(r.y + r.height) * v.height / u32::from(v.rows.max(1));
    PixelRect::new(x as u16, y as u16, (right - x) as u16, (bottom - y) as u16)
}
fn menu_rect(menu: &crate::shelf::ContextMenu, v: Viewport) -> Rect {
    let label_width = menu
        .items
        .iter()
        .map(|item| starkit::ratatui::text::Line::from(item.label.as_str()).width())
        .max()
        .unwrap_or(0);
    let width = (label_width as u16 + 4)
        .max(22)
        .min(v.columns.saturating_sub(2));
    let height = (menu.items.len() as u16 + 2).min(v.rows.saturating_sub(2));
    Rect {
        x: menu.position[0].min(v.columns.saturating_sub(width + 1)),
        y: menu.position[1].min(v.rows.saturating_sub(height + 1)),
        width,
        height,
    }
}
fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height
}
impl Workspace {
    pub(super) fn cell_menu_bounds(&self) -> Option<Rect> {
        self.context_menu
            .as_ref()
            .map(|menu| menu_rect(menu, self.viewport))
    }
    pub(super) fn cells_overlay(&self) -> bool {
        self.modal.is_some() || self.context_menu.is_some() || self.git_open
    }
    pub(super) fn presentation_canvas(&self) -> PixelRect {
        if self.cells {
            pixels(
                self.viewport,
                Layout::new(self.viewport, self.cell_ui.sidebar).canvas,
            )
        } else {
            crate::shelf::Layout::new(
                self.viewport.width as u16,
                self.viewport.height as u16,
                self.state.space,
            )
            .canvas
        }
    }
    pub(super) fn cell_panes(&self) -> Vec<(usize, Rect)> {
        let ids: Vec<_> = self
            .state
            .visible()
            .iter()
            .map(|id| self.config.canonical_pane(*id))
            .collect();
        Layout::new(self.viewport, self.cell_ui.sidebar).panes(
            &ids,
            self.config.canonical_pane(self.state.selected()),
            self.state.split_ratio,
        )
    }
    pub(super) fn cell_pane_at(&self, x: u16, y: u16) -> Option<usize> {
        self.cell_panes()
            .iter()
            .find(|(_, r)| contains(*r, x, y))
            .map(|(id, _)| *id)
    }
    pub(super) fn cell_visible_panes(&self) -> Vec<(usize, PixelRect)> {
        let canvas = self.presentation_canvas();
        self.cell_panes()
            .into_iter()
            .map(|(id, r)| {
                let p = pixels(self.viewport, r);
                (
                    id,
                    PixelRect::new(p.x - canvas.x, p.y - canvas.y, p.width, p.height),
                )
            })
            .collect()
    }
    fn cell_button(&mut self, b: &mut Buffer, r: Rect, label: &str, action: String, active: bool) {
        let focused = (!self.cell_ui.terminal_focus
            && self.cell_ui.focus == self.cell_ui.controls.len())
            || self.cell_ui.hover.is_some_and(|[x, y]| contains(r, x, y));
        let t = &self.appearance;
        let style = Style::default()
            .fg(color(if active || focused { &t.accent } else { &t.fg }))
            .bg(color(if focused { &t.selection } else { &t.panel }));
        put(b, r.x, r.y, r.width, label, style);
        self.cell_ui.controls.push(Control { action, rect: r });
    }
    pub(super) fn draw_cells(&mut self, v: Viewport) -> Scene {
        self.viewport = v;
        let layout = Layout::new(v, self.cell_ui.sidebar);
        let canvas = pixels(v, layout.canvas);
        self.settings.width = u32::from(canvas.width).max(1);
        self.settings.height = u32::from(canvas.height).max(1);
        self.settings.fps = self.terminal_fps;
        self.cell_ui.controls.clear();
        let base = Style::default()
            .fg(color(&self.appearance.fg))
            .bg(color(&self.appearance.bg));
        let accent = base
            .fg(color(&self.appearance.accent))
            .add_modifier(Modifier::BOLD);
        let muted = base.fg(color(&self.appearance.muted));
        let mut b = Buffer::empty(CellRect::new(0, 0, v.columns, v.rows));
        b.set_style(b.area, base);
        put(
            &mut b,
            0,
            0,
            v.columns,
            &format!(
                "STAR/FLUX  {}  {}  {}",
                self.config
                    .label
                    .as_deref()
                    .unwrap_or(&self.config.workspace),
                self.config.host.as_deref().unwrap_or("local"),
                self.config.directory.as_deref().unwrap_or("")
            ),
            accent,
        );
        if layout.small {
            put(
                &mut b,
                0,
                v.rows / 2,
                v.columns,
                "Resize to at least 60 columns x 16 rows. Sessions remain running.",
                accent,
            );
        } else {
            let mut buttons = vec![
                ("Projects", "cells:sidebar"),
                ("Close project", "project:close"),
                ("+Tab", "terminal:new"),
                (
                    if self.state.split { "Unsplit" } else { "Split" },
                    "terminal:split",
                ),
                ("Git", "git:details"),
                ("Theme", "theme:next"),
                ("File", "file:deliver"),
                ("Refresh", "ui:refresh"),
                ("Tab menu", "cells:tab-menu"),
                ("Project menu", "cells:project-menu"),
                ("Order", "projects:sort:mode"),
                (
                    if self.navigation.high_first {
                        "High"
                    } else {
                        "Low"
                    },
                    "projects:sort:direction",
                ),
            ];
            if self.state.split {
                buttons.extend([("<", "cells:split:left"), (">", "cells:split:right")]);
            }
            let (mut x, mut y) = (0, 1);
            for (label, action) in buttons {
                let label = format!("[{label}]");
                let width = starkit::ratatui::text::Line::from(label.as_str()).width() as u16;
                if x + width > v.columns {
                    x = 0;
                    y += 1;
                }
                if y > 2 {
                    break;
                }
                self.cell_button(
                    &mut b,
                    Rect {
                        x,
                        y,
                        width,
                        height: 1,
                    },
                    &label,
                    action.into(),
                    false,
                );
                x += width + 1;
            }
            // Scroll the tab strip to keep the selected tab visible.
            let ids = self.state.terminal_ids();
            let selected = ids
                .iter()
                .position(|id| *id == self.state.terminal)
                .unwrap_or(0);
            self.cell_ui.tab_offset = self.cell_ui.tab_offset.min(selected);
            let widths: BTreeMap<_, _> = ids
                .iter()
                .map(|id| {
                    (
                        *id,
                        starkit::ratatui::text::Line::from(self.terminal_label(*id))
                            .width()
                            .clamp(8, 24) as u16
                            + 3,
                    )
                })
                .collect();
            let tab_width = |id| widths[&id];
            while ids
                .iter()
                .skip(self.cell_ui.tab_offset)
                .take(selected + 1 - self.cell_ui.tab_offset)
                .map(|id| tab_width(*id) + 1)
                .sum::<u16>()
                > v.columns.saturating_sub(4)
                && self.cell_ui.tab_offset < selected
            {
                self.cell_ui.tab_offset += 1;
            }
            let mut x = if self.cell_ui.tab_offset > 0 { 2 } else { 0 };
            if x > 0 {
                self.cell_button(
                    &mut b,
                    Rect {
                        x: 0,
                        y: 3,
                        width: 2,
                        height: 1,
                    },
                    "<",
                    "cells:tabs:left".into(),
                    false,
                );
            }
            for (index, id) in ids.iter().enumerate().skip(self.cell_ui.tab_offset) {
                let width = tab_width(*id).min(v.columns.saturating_sub(x + 2));
                if width < 4 {
                    break;
                }
                let label = format!(
                    " {}{} ",
                    if *id == self.state.terminal { ">" } else { "" },
                    self.terminal_label(*id)
                );
                self.cell_button(
                    &mut b,
                    Rect {
                        x,
                        y: 3,
                        width,
                        height: 1,
                    },
                    &label,
                    format!("terminal:{id}"),
                    *id == self.state.terminal,
                );
                x += width + 1;
                if x + 5 >= v.columns && index + 1 < ids.len() {
                    self.cell_button(
                        &mut b,
                        Rect {
                            x: v.columns - 2,
                            y: 3,
                            width: 2,
                            height: 1,
                        },
                        ">",
                        "cells:tabs:right".into(),
                        false,
                    );
                    break;
                }
            }
            if layout.sidebar.width > 0 {
                self.project_page_size = usize::from(layout.sidebar.height.saturating_sub(2));
                self.project_offset = self.project_offset.min(
                    self.projects
                        .len()
                        .saturating_sub(self.project_page_size.max(1)),
                );
                let title = format!(
                    "{} / {}",
                    if self.navigation.recent {
                        "Activity"
                    } else {
                        "Manual"
                    },
                    self.projects.len()
                );
                put(&mut b, 0, 4, 24, &title, muted);
                for n in 0..self.project_page_size {
                    let Some(project) = self.projects.get(self.project_offset + n) else {
                        break;
                    };
                    let a = self.project_activity(project);
                    let mark = if a.questions > 0 {
                        "?"
                    } else if a.working > 0 {
                        "*"
                    } else if a.waiting > 0 {
                        "!"
                    } else {
                        " "
                    };
                    let label = format!(
                        "{mark} {}",
                        project.label.as_deref().unwrap_or(&project.workspace)
                    );
                    let action = format!("project:{}", project.workspace);
                    let active = project.workspace == self.config.workspace;
                    self.cell_button(
                        &mut b,
                        Rect {
                            x: 0,
                            y: 5 + n as u16,
                            width: layout.sidebar.width,
                            height: 1,
                        },
                        &label,
                        action.clone(),
                        active,
                    );
                    let project_id = action.strip_prefix("project:").unwrap();
                    self.cell_button(
                        &mut b,
                        Rect {
                            x: 22,
                            y: 5 + n as u16,
                            width: 2,
                            height: 1,
                        },
                        "x",
                        format!("close-project:{project_id}"),
                        false,
                    );
                }
                self.cell_button(
                    &mut b,
                    Rect {
                        x: 0,
                        y: v.rows - 3,
                        width: 24,
                        height: 1,
                    },
                    "[+ Add project]",
                    "project:new".into(),
                    false,
                );
                for y in 4..v.rows - 2 {
                    put(&mut b, 24, y, 1, "│", muted);
                }
            } else {
                self.project_page_size = 0;
            }
            let panes = self.cell_panes();
            if panes.len() == 2 {
                let x = panes[0].1.x + panes[0].1.width;
                for y in layout.canvas.y..layout.canvas.y + layout.canvas.height {
                    put(&mut b, x, y, 1, "│", muted);
                }
            }
            if panes.is_empty() {
                put(
                    &mut b,
                    layout.canvas.x + 1,
                    5,
                    layout.canvas.width.saturating_sub(2),
                    "No terminal open. Choose +Tab.",
                    muted,
                );
            }
        }
        self.cell_ui.focus = self
            .cell_ui
            .focus
            .min(self.cell_ui.controls.len().saturating_sub(1));
        let summary = self
            .git_status()
            .map(|s| s.summary())
            .unwrap_or_else(|| self.status.clone());
        put(
            &mut b,
            0,
            v.rows.saturating_sub(2),
            v.columns,
            &format!("{} | {summary}", self.status),
            muted,
        );
        let hint = if self.cell_ui.terminal_focus {
            "Ctrl+Esc / F6: controls | mouse: focus/select | sessions persist on exit"
        } else {
            "Tab/Enter: controls  +: add project  x: close project  Esc/F6: terminal"
        };
        put(&mut b, 0, v.rows.saturating_sub(1), v.columns, hint, accent);
        if self.context_menu.is_some() && !layout.small {
            for (pane, rect) in self.cell_panes() {
                if let Some(terminal) = self.terminals.get(&(self.config.workspace.clone(), pane)) {
                    terminal.paint_cells(&mut b, rect, &self.appearance);
                }
            }
        }
        if self.cells_overlay() && !layout.small {
            self.draw_cell_overlay(&mut b, base, accent);
        }
        let mut scene = Scene::from_buffer(&b, v, 0);
        scene.background = self.appearance.bg.clone();
        scene.foreground = self.appearance.fg.clone();
        scene.accent = self.appearance.accent.clone();
        scene.border = self.appearance.border.clone();
        scene.pointer_regions = self
            .cell_ui
            .controls
            .iter()
            .map(|control| control.rect)
            .collect();
        if !layout.small
            && let Some(menu) = &self.context_menu
        {
            scene.components.push(Component::Menu {
                rect: menu_rect(menu, v),
            });
        }
        if !self.cells_overlay() {
            for (_, rect) in self.cell_panes() {
                scene.components.push(Component::Terminal { rect });
            }
        }
        scene
    }
    fn draw_cell_overlay(&mut self, b: &mut Buffer, base: Style, accent: Style) {
        let area = b.area;
        // Dialogs own the screen. Context menus preserve the cell-composed
        // terminal behind their small popup, with input captured by the menu.
        if self.context_menu.is_none() {
            for cell in &mut b.content {
                cell.reset();
                cell.set_style(base);
            }
        }
        self.cell_ui.controls.clear();
        if let Some(modal) = &self.modal {
            let title = match &modal.kind {
                Prompt::Deliver => "Deliver a file (path on workspace host)",
                Prompt::Project => "Add / reopen project",
                Prompt::RenameProject(_) => "Rename project",
                Prompt::RenameTab(_) => "Rename terminal tab",
                Prompt::RenameAgent(_) => "Rename agent",
                Prompt::Launch | Prompt::RunCommand => "App view unavailable",
            };
            put(b, 1, 1, area.width.saturating_sub(2), title, accent);
            for (i, (label, field)) in modal.fields.iter().enumerate() {
                let y = 3 + i as u16 * 3;
                put(b, 1, y, area.width.saturating_sub(2), label, base);
                let focused = i == modal.selected;
                // Slice by character boundary and keep the edit cursor visible.
                let before = &field.text()[..field.cursor()];
                let tail: String = before
                    .chars()
                    .rev()
                    .take(usize::from(area.width.saturating_sub(5)))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let value = if focused {
                    format!("{tail}│{}", &field.text()[field.cursor()..])
                } else {
                    field.text().to_owned()
                };
                put(
                    b,
                    2,
                    y + 1,
                    area.width.saturating_sub(4),
                    &value,
                    if focused { accent } else { base },
                );
            }
            let error = modal.error.clone();
            put(
                b,
                1,
                area.height.saturating_sub(4),
                area.width.saturating_sub(2),
                &error,
                base.fg(color(&self.appearance.error)),
            );
            self.cell_button(
                b,
                Rect {
                    x: 1,
                    y: area.height - 3,
                    width: 10,
                    height: 1,
                },
                "[Accept]",
                "modal:accept".into(),
                false,
            );
            self.cell_button(
                b,
                Rect {
                    x: 13,
                    y: area.height - 3,
                    width: 10,
                    height: 1,
                },
                "[Cancel]",
                "modal:cancel".into(),
                false,
            );
            put(
                b,
                1,
                area.height - 1,
                area.width.saturating_sub(2),
                "Tab: next field  Ctrl+Enter: accept  Esc: cancel",
                accent,
            );
        } else if let Some(menu) = &self.context_menu {
            use starkit::ratatui::widgets::{Block, Borders, Clear, Widget};
            let r = menu_rect(menu, self.viewport);
            let area = CellRect::new(r.x, r.y, r.width, r.height);
            Clear.render(area, b);
            Block::default()
                .borders(Borders::ALL)
                .style(base.bg(color(&self.appearance.panel)))
                .border_style(base.fg(color(&self.appearance.border)))
                .render(area, b);
            for (i, item) in menu
                .items
                .iter()
                .enumerate()
                .take(usize::from(r.height.saturating_sub(2)))
            {
                let rect = Rect {
                    x: r.x + 1,
                    y: r.y + 1 + i as u16,
                    width: r.width.saturating_sub(2),
                    height: 1,
                };
                let highlighted = menu.selected == Some(i);
                let style = base.bg(color(if highlighted {
                    &self.appearance.selection
                } else {
                    &self.appearance.panel
                }));
                b.set_style(CellRect::new(rect.x, rect.y, rect.width, 1), style);
                put(
                    b,
                    rect.x + 1,
                    rect.y,
                    rect.width.saturating_sub(2),
                    &item.label,
                    style,
                );
                self.cell_ui.controls.push(Control {
                    action: format!("menu:item:{i}"),
                    rect,
                });
            }
        } else if self.git_open {
            put(
                b,
                1,
                1,
                area.width.saturating_sub(2),
                "Working tree",
                accent,
            );
            if let Some(git) = self.git_status() {
                let lines = [
                    git.summary(),
                    format!("Root: {}", git.root),
                    format!("Upstream: {}  Stashes: {}", git.upstream, git.stash),
                    format!("Last commit: {}", git.last_commit),
                    git.error.clone(),
                ];
                for (i, line) in lines.iter().enumerate() {
                    put(b, 1, 3 + i as u16, area.width.saturating_sub(2), line, base);
                }
                for (i, file) in git
                    .files
                    .iter()
                    .skip(self.git_scroll)
                    .take(usize::from(area.height.saturating_sub(12)))
                    .enumerate()
                {
                    put(
                        b,
                        1,
                        9 + i as u16,
                        area.width.saturating_sub(2),
                        &format!(
                            "{}{} {}{}",
                            file.index,
                            file.worktree,
                            if file.conflict { "CONFLICT " } else { "" },
                            file.path
                        ),
                        base,
                    );
                }
            }
            self.cell_button(
                b,
                Rect {
                    x: 1,
                    y: area.height - 2,
                    width: 10,
                    height: 1,
                },
                "[Refresh]",
                "git:refresh".into(),
                false,
            );
            self.cell_button(
                b,
                Rect {
                    x: 13,
                    y: area.height - 2,
                    width: 10,
                    height: 1,
                },
                "[Close]",
                "git:close".into(),
                false,
            );
            put(
                b,
                1,
                area.height - 1,
                area.width.saturating_sub(2),
                "Up/Down/PgUp/PgDn: scroll  r: refresh  Esc: close",
                accent,
            );
        }
    }
    fn cell_activate(&mut self, action: &str) -> Result<()> {
        match action {
            "cells:sidebar" => self.cell_ui.sidebar = !self.cell_ui.sidebar,
            "cells:tab-menu" => {
                let target = format!("terminal:{}", self.state.terminal);
                let anchor = self
                    .cell_ui
                    .controls
                    .iter()
                    .find(|c| c.action == target)
                    .map_or([0, 4], |c| [c.rect.x, c.rect.y + 1]);
                self.open_context_menu(Some(&target), anchor);
                if let Some(menu) = &mut self.context_menu {
                    menu.selected = Some(0);
                }
            }
            "cells:project-menu" => {
                let target = format!("project:{}", self.config.workspace);
                let anchor = self
                    .cell_ui
                    .controls
                    .iter()
                    .find(|c| c.action == target)
                    .map_or([0, 3], |c| [c.rect.x, c.rect.y + 1]);
                self.open_context_menu(Some(&target), anchor);
                if let Some(menu) = &mut self.context_menu {
                    menu.selected = Some(0);
                }
            }
            "cells:tabs:left" | "cells:tabs:right" => {
                let ids = self.state.terminal_ids();
                let index = ids
                    .iter()
                    .position(|id| *id == self.state.terminal)
                    .unwrap_or(0);
                let next = if action.ends_with("left") {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(ids.len().saturating_sub(1))
                };
                if let Some(id) = ids.get(next) {
                    self.action(&format!("terminal:{id}"))?;
                }
            }
            "cells:split:left" | "cells:split:right" => {
                self.state.split_ratio = (self.state.split_ratio
                    + if action.ends_with("left") {
                        -0.05
                    } else {
                        0.05
                    })
                .clamp(0.2, 0.8);
                self.save();
            }
            _ => self.action(action)?,
        }
        Ok(())
    }
    fn cell_reorder(&mut self, id: &str, gap: usize) -> Result<()> {
        if !self.navigation.recent && self.navigation.reorder(id, gap) {
            self.navigation.save(&self.controller_id)?;
            self.sort_projects(false);
        }
        Ok(())
    }
    pub(super) fn cells_input(&mut self, input: Input) -> Result<()> {
        match input {
            Input::Resize { viewport } => {
                self.viewport = viewport;
                self.cell_ui.hover = None;
                self.native_layout_key.clear();
            }
            Input::Notice { message } => {
                if let Some(action) = message.strip_prefix("action:") {
                    self.cell_activate(action)?;
                } else if message == "focus-canvas:terminals" && !self.cells_overlay() {
                    self.cell_ui.terminal_focus = true;
                } else if let Some(paths) = message.strip_prefix("drop:") {
                    self.deliver(self.state.selected(), serde_json::from_str(paths)?);
                }
            }
            Input::Osc72 { text } => self.drop_message(&text)?,
            Input::Paste { text } => {
                if let Some(modal) = &mut self.modal {
                    modal.fields[modal.selected].1.paste(&text);
                } else if self.cell_ui.terminal_focus && !self.cells_overlay() {
                    self.paste(text);
                }
            }
            Input::Key { code, modifiers } => {
                if self.modal.is_some() {
                    self.modal_key(code, modifiers)?;
                    return Ok(());
                }
                if self.context_menu.is_some() {
                    self.menu_key(&code)?;
                    return Ok(());
                }
                if self.git_open {
                    match code.as_str() {
                        "escape" => self.git_open = false,
                        "up" | "pageup" => {
                            self.git_scroll =
                                self.git_scroll
                                    .saturating_sub(if code == "up" { 1 } else { 10 })
                        }
                        "down" | "pagedown" => {
                            self.git_scroll =
                                (self.git_scroll + if code == "down" { 1 } else { 10 }).min(
                                    self.git_status()
                                        .map_or(0, |g| g.files.len().saturating_sub(1)),
                                )
                        }
                        "char:r" => self.refresh_git(true),
                        _ => {}
                    }
                    return Ok(());
                }
                if (code == "escape" && modifiers & 2 != 0) || matches!(code.as_str(), "f6" | "f:6")
                {
                    self.cell_ui.terminal_focus = if code == "escape" {
                        false
                    } else {
                        !self.cell_ui.terminal_focus
                    };
                    self.release();
                    return Ok(());
                }
                if self.cell_ui.terminal_focus {
                    self.direct_input(EmbeddedKind::Key {
                        code,
                        modifiers,
                        phase: KeyPhase::Press,
                    });
                } else {
                    match code.as_str() {
                        "escape" => self.cell_ui.terminal_focus = true,
                        "tab" | "backtab" => {
                            let count = self.cell_ui.controls.len().max(1);
                            self.cell_ui.focus = if code == "backtab" || modifiers & 1 != 0 {
                                (self.cell_ui.focus + count - 1) % count
                            } else {
                                (self.cell_ui.focus + 1) % count
                            };
                        }
                        "enter" => {
                            if let Some(control) =
                                self.cell_ui.controls.get(self.cell_ui.focus).cloned()
                            {
                                self.cell_activate(&control.action)?;
                            }
                        }
                        "f2" | "f:2" => self.open_prompt(Prompt::RenameTab(self.state.terminal)),
                        "char:q" => self.quit = true,
                        "char:n" => self.action("terminal:new")?,
                        "char:p" | "char:+" => self.action("project:new")?,
                        "char:x" => self.action("project:close")?,
                        "char:g" => self.action("git:details")?,
                        "char:t" => self.cycle_theme()?,
                        "char:f" => self.action("file:deliver")?,
                        "char:[" | "char:]" => {
                            if let Some(i) = self
                                .projects
                                .iter()
                                .position(|p| p.workspace == self.config.workspace)
                            {
                                let next = if code == "char:[" {
                                    (i + self.projects.len() - 1) % self.projects.len()
                                } else {
                                    (i + 1) % self.projects.len()
                                };
                                self.switch_project(&self.projects[next].workspace.clone())?;
                            }
                        }
                        "left" | "right" if modifiers & 2 != 0 => {
                            self.cell_activate(if code == "left" {
                                "cells:split:left"
                            } else {
                                "cells:split:right"
                            })?;
                        }
                        "up" | "down" if modifiers & 2 != 0 => {
                            let id = self
                                .cell_ui
                                .controls
                                .get(self.cell_ui.focus)
                                .and_then(|c| c.action.strip_prefix("project:"))
                                .unwrap_or(&self.config.workspace)
                                .to_owned();
                            if let Some(index) =
                                self.projects.iter().position(|p| p.workspace == id)
                            {
                                self.cell_reorder(
                                    &id,
                                    if code == "up" {
                                        index.saturating_sub(1)
                                    } else {
                                        index + 2
                                    },
                                )?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Input::Pointer {
                action,
                button,
                x,
                y,
                modifiers,
                ..
            } => {
                let hit = self
                    .cell_ui
                    .controls
                    .iter()
                    .rfind(|c| contains(c.rect, x, y))
                    .cloned();
                self.cell_ui.hover = Some([x, y]);
                if let Some(menu) = &mut self.context_menu {
                    if action == "move" {
                        menu.selected = hit
                            .as_ref()
                            .and_then(|c| c.action.strip_prefix("menu:item:"))
                            .and_then(|n| n.parse().ok());
                    } else if action == "down" {
                        if button == 0
                            && let Some(control) =
                                hit.filter(|c| c.action.starts_with("menu:item:"))
                        {
                            self.cell_activate(&control.action)?;
                        } else if !contains(menu_rect(menu, self.viewport), x, y) {
                            self.context_menu = None;
                        }
                    }
                    return Ok(());
                }
                if action == "down"
                    && let Some(control) = &hit
                {
                    if button == 1 {
                        self.open_context_menu(Some(&control.action), [x, y]);
                    } else {
                        self.cell_ui.focus = self
                            .cell_ui
                            .controls
                            .iter()
                            .position(|c| c.action == control.action)
                            .unwrap_or(0);
                        self.cell_ui.terminal_focus = false;
                        self.cell_ui.drag_project =
                            control.action.strip_prefix("project:").map(str::to_owned);
                        self.cell_activate(&control.action)?;
                    }
                    return Ok(());
                }
                if action == "up"
                    && let Some(id) = self.cell_ui.drag_project.take()
                {
                    if let Some(target) =
                        hit.and_then(|c| c.action.strip_prefix("project:").map(str::to_owned))
                        && let Some(index) =
                            self.projects.iter().position(|p| p.workspace == target)
                    {
                        self.cell_reorder(&id, index)?;
                    }
                    return Ok(());
                }
                if self.cells_overlay() {
                    return Ok(());
                }
                let layout = Layout::new(self.viewport, self.cell_ui.sidebar);
                if x < layout.sidebar.width && action.starts_with("scroll_") {
                    self.action(if action == "scroll_up" {
                        "projects:scroll:up"
                    } else {
                        "projects:scroll:down"
                    })?;
                    return Ok(());
                }
                let panes = self.cell_panes();
                let pane = self.cell_pane_at(x, y);
                if action == "down"
                    && let Some(id) = pane
                {
                    if button == 1 {
                        self.open_context_menu(Some(&format!("terminal:{id}")), [x, y]);
                        return Ok(());
                    }
                    self.state.select_terminal(id);
                    self.cell_ui.pointer_pane = Some(id);
                    self.cell_ui.terminal_focus = true;
                }
                if action == "down"
                    && pane.is_none()
                    && panes.len() == 2
                    && contains(layout.canvas, x, y)
                {
                    self.drag_split = true;
                    return Ok(());
                }
                if self.drag_split {
                    if action == "up" {
                        self.drag_split = false;
                        self.save();
                    } else {
                        self.state.split_ratio = (f64::from(x.saturating_sub(layout.canvas.x))
                            / f64::from(layout.canvas.width.max(1)))
                        .clamp(0.2, 0.8);
                    }
                    return Ok(());
                }
                if let Some((id, rect)) = self
                    .cell_ui
                    .pointer_pane
                    .or(pane)
                    .and_then(|id| panes.iter().find(|(p, _)| *p == id))
                    .copied()
                {
                    let cell = [
                        (self.viewport.width / u32::from(self.viewport.columns.max(1))).max(1),
                        (self.viewport.height / u32::from(self.viewport.rows.max(1))).max(1),
                    ];
                    let (events, _) = self.text_gesture.route(crate::selection::Pointer {
                        action: action.clone(),
                        button,
                        position: [
                            f64::from(x.saturating_sub(rect.x) * cell[0] as u16),
                            f64::from(y.saturating_sub(rect.y) * cell[1] as u16),
                        ],
                        modifiers,
                    });
                    if let Some(terminal) =
                        self.terminals.get_mut(&(self.config.workspace.clone(), id))
                    {
                        for event in events {
                            terminal.pointer(
                                if event.action == "drag" {
                                    "move"
                                } else {
                                    &event.action
                                },
                                event.button,
                                event.position,
                                event.modifiers,
                            )?;
                        }
                    }
                }
                if action == "up" {
                    self.cell_ui.pointer_pane = None;
                }
            }
            Input::CancelPointer => {
                self.cell_ui.hover = None;
                self.release();
                self.cell_ui.pointer_pane = None;
                self.cell_ui.drag_project = None;
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cell_layout_preserves_every_terminal_cell() {
        for (cols, rows) in [(60, 16), (80, 24), (100, 30), (200, 60)] {
            let v = Viewport {
                columns: cols,
                rows,
                width: u32::from(cols) * 9,
                height: u32::from(rows) * 18,
                generation: 1,
            };
            let l = Layout::new(v, false);
            assert!(!l.small);
            assert_eq!(l.sidebar.width > 0, cols >= 100);
            let panes = l.panes(&[1, 2], 2, 0.5);
            assert_eq!(panes.len(), 2);
            assert_eq!(panes[0].1.width + panes[1].1.width + 1, l.canvas.width);
            assert_eq!(panes[1].1.x + panes[1].1.width, cols);
            assert_eq!(pixels(v, l.canvas).height, (rows - 6) * 18);
        }
    }
    #[test]
    fn tiny_layout_suspends_presentation_not_sessions() {
        let l = Layout::new(
            Viewport {
                columns: 30,
                rows: 10,
                ..Viewport::default()
            },
            true,
        );
        assert!(l.small);
        assert!(l.panes(&[1, 2], 1, 0.5).is_empty());
    }
}
