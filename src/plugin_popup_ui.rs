use super::*;
use crate::plugin_popup::{Options, Reservation};
use starkit::native_surface::{Primitive, Surface};
pub(super) struct Popup {
    reservation: Reservation,
    terminal: crate::terminal::Terminal,
    birth: u64,
    terminal_focus: bool,
    app_keyboard_released: bool,
    controls: bool,
}
impl Drop for Popup {
    fn drop(&mut self) {
        self.terminal.stop_transient_group(self.birth);
    }
}
fn pixels(rect: Rect, v: Viewport) -> PixelRect {
    let x = u32::from(rect.x) * v.width / u32::from(v.columns.max(1));
    let y = u32::from(rect.y) * v.height / u32::from(v.rows.max(1));
    let right = u32::from(rect.x + rect.width) * v.width / u32::from(v.columns.max(1));
    let bottom = u32::from(rect.y + rect.height) * v.height / u32::from(v.rows.max(1));
    PixelRect::new(x as u16, y as u16, (right - x) as u16, (bottom - y) as u16)
}
impl Workspace {
    fn popup_area(&self) -> Rect {
        let v = self.viewport;
        if v.columns < 60 || v.rows < 16 {
            return Rect {
                x: 0,
                y: 0,
                width: v.columns,
                height: v.rows,
            };
        }
        let p = self.presentation_canvas();
        Rect {
            x: (u32::from(p.x) * u32::from(v.columns) / v.width.max(1)) as u16,
            y: (u32::from(p.y) * u32::from(v.rows) / v.height.max(1)) as u16,
            width: ((u32::from(p.width) * u32::from(v.columns) / v.width.max(1)) as u16).max(1),
            height: ((u32::from(p.height) * u32::from(v.rows) / v.height.max(1)) as u16).max(1),
        }
    }
    pub(super) fn open_popup(
        &mut self,
        plugin: &str,
        entrypoint: &str,
        digest: &str,
        options: Options,
    ) -> Result<Value> {
        ensure!(
            self.desktop.is_none() && self.native_terminal_cells,
            "A native terminal frontend is required for popup terminals"
        );
        if self.frontend_command_busy() {
            return Err(crate::automation::fault(
                "frontend_busy",
                "A popup, dialog, menu, navigation or history review is active",
            ));
        }
        options.bounds(self.popup_area())?;
        crate::plugin_panes::launcher()?;
        let owner = crate::frontend_commands::current(&self.controller_id, &self.workspace_id)?;
        let reservation = Reservation::create(
            owner,
            &self.config,
            self.state.selected(),
            plugin,
            entrypoint,
            digest,
            options,
        )?;
        reservation.record.options.bounds(self.popup_area())?;
        let path = crate::terminal::socket(&format!("popup-{}", reservation.record.id), 0)?;
        let mut terminal = crate::terminal::Terminal::spawn(&reservation.argv()?, path)?;
        terminal.set_palette(&self.appearance);
        let birth =
            crate::agent_state::process_start(terminal.pid.context("Popup process has no PID")?)
                .context("Popup process identity unavailable")?;
        let result =
            json!({"id":reservation.record.id,"kind":"popup","acknowledgement":"pty_started"});
        let terminal_focus = self.cell_ui.terminal_focus;
        let app_keyboard_released = self.app_keyboard_released;
        self.release();
        self.popup = Some(Popup {
            reservation,
            terminal,
            birth,
            terminal_focus,
            app_keyboard_released,
            controls: false,
        });
        self.native_layout_key.clear();
        self.note(self.keyboard.popup_controls_hint().map_or_else(
            || "Plugin popup · close with header ×".into(),
            |key| format!("Plugin popup · {key}: close controls"),
        ));
        Ok(result)
    }
    pub(super) fn close_popup(&mut self, id: &str) -> Result<()> {
        let popup = self
            .popup
            .as_ref()
            .ok_or_else(|| crate::automation::fault("stale_popup", "No plugin popup is open"))?;
        if popup.reservation.record.id != id {
            return Err(crate::automation::fault(
                "stale_popup",
                "Popup identity changed; current popup was left running",
            ));
        }
        let popup = self.popup.take().unwrap();
        self.cell_ui.terminal_focus = popup.terminal_focus;
        // Opening released the compositor's keyboard focus. Reacquire the
        // selected app on its next key, even if it was captured before the
        // popup; restoring a stale false flag would skip that focus request.
        self.app_keyboard_released = self.state.space == Space::Apps || popup.app_keyboard_released;
        drop(popup);
        self.native_layout_key.clear();
        for t in self.terminals.values_mut() {
            t.reset_native();
        }
        self.note("Plugin popup closed".into());
        Ok(())
    }
    pub(super) fn popup_tick(&mut self) {
        let area = self.popup_area();
        let v = self.viewport;
        let Some(p) = &mut self.popup else {
            return;
        };
        let outcome = (|| -> Result<()> {
            ensure!(
                p.reservation.current()
                    && p.reservation.record.project.session
                        == crate::session_catalog::group(&self.config),
                "Popup owner/session changed"
            );
            let (_, inner) = p.reservation.record.options.bounds(area)?;
            p.terminal.poll_transient(p.birth)?;
            p.terminal.set_metrics(self.typography);
            p.terminal.resize_native(
                inner,
                [0, 0],
                [
                    (v.width / u32::from(v.columns.max(1))).max(1) as u16,
                    (v.height / u32::from(v.rows.max(1))).max(1) as u16,
                ],
            )?;
            if let Some(frame) = p.terminal.native_frame(
                format!("plugin-popup-{}", p.reservation.record.id),
                v.generation,
                inner,
                &self.appearance,
                true,
            ) {
                self.effects.push(ServerMessage::TerminalCells { frame });
            }
            if let Some(text) = p.terminal.take_clipboard() {
                self.effects.push(ServerMessage::Clipboard { text });
            }
            Ok(())
        })();
        if p.terminal.exited || outcome.is_err() {
            let id = p.reservation.record.id.clone();
            let error = outcome
                .err()
                .map(|e| format!("{e:#}"))
                .or_else(|| crate::plugin_popup::get(&id).ok().and_then(|r| r.error));
            let _ = self.close_popup(&id);
            if let Some(e) = error {
                self.note(format!("Popup: {e}"));
            }
        }
    }
    pub(super) fn popup_snapshot(&self) -> Value {
        self.popup.as_ref().map_or(Value::Null,|p|{let bounds=p.reservation.record.options.bounds(self.popup_area()).ok();json!({"id":p.reservation.record.id,"plugin":p.reservation.record.plugin,"entrypoint":p.reservation.record.entrypoint.id,"title":p.reservation.record.entrypoint.title,"rect":bounds.map(|b|b.0),"terminal_rect":bounds.map(|b|b.1),"process":p.terminal.pid,"controls_focused":p.controls})})
    }
    pub(super) fn popup_input(&mut self, input: &Input) -> bool {
        let area = self.popup_area();
        let v = self.viewport;
        let Some(p) = &mut self.popup else {
            return false;
        };
        let Ok((outer, inner)) = p.reservation.record.options.bounds(area) else {
            return true;
        };
        if let Input::Notice { message } = input {
            if message == "terminal-repaint-key" {
                if !p.controls
                    && let Err(error) = p.terminal.key_raw("char:l", 2)
                {
                    self.note(format!("Popup input: {error:#}"));
                }
                return true;
            }
            if message == "window-focus:lost" {
                p.terminal.release();
            }
            return false;
        }
        let key = match input {
            Input::Key { code, modifiers } => Some((code, *modifiers, KeyPhase::Press)),
            Input::Embedded { event, .. } => match &event.kind {
                EmbeddedKind::Key {
                    code,
                    modifiers,
                    phase,
                } => Some((code, *modifiers, *phase)),
                _ => None,
            },
            _ => None,
        };
        if let Some((code, modifiers, phase)) = key {
            if phase == KeyPhase::Release {
                return true;
            }
            if self.keyboard.popup_controls(code, modifiers) {
                p.controls = true;
                p.terminal.release();
                return true;
            }
            if p.controls {
                if code == "escape" && modifiers == 0 {
                    p.controls = false;
                } else if code == "enter" && modifiers == 0 && phase == KeyPhase::Press {
                    let id = p.reservation.record.id.clone();
                    let _ = self.close_popup(&id);
                }
                return true;
            }
        }
        let mut close = false;
        let result = match input {
            Input::Key { code, modifiers } => p.terminal.key_raw(code, *modifiers),
            Input::Paste { text } if !p.controls => p.terminal.paste_raw(text),
            Input::Embedded { canvas, event } => match &event.kind {
                EmbeddedKind::Key {
                    code,
                    modifiers,
                    phase,
                } if *phase != KeyPhase::Release => p.terminal.key_raw(code, *modifiers),
                EmbeddedKind::Text { text } if !p.controls => p.terminal.paste_raw(text),
                EmbeddedKind::Pointer {
                    action,
                    button,
                    position,
                    modifiers,
                } if canvas == "plugin-popup" && (!p.controls || action == "down") => {
                    if action == "down" {
                        p.controls = false;
                    }
                    p.terminal.pointer(action, *button, *position, *modifiers)
                }
                EmbeddedKind::ReleaseAll => {
                    p.terminal.release();
                    Ok(())
                }
                _ => Ok(()),
            },
            Input::Pointer {
                action,
                button,
                x,
                y,
                pixel,
                modifiers,
            } => {
                let inside = *x >= inner.x
                    && *x < inner.x + inner.width
                    && *y >= inner.y
                    && *y < inner.y + inner.height;
                close = action == "down"
                    && *button == 0
                    && *y == outer.y
                    && *x >= outer.x + outer.width - 3
                    && *x < outer.x + outer.width;
                if inside && (!p.controls || action == "down") {
                    if action == "down" {
                        p.controls = false;
                    }
                    let r = pixels(inner, v);
                    let xy = pixel.map_or(
                        [
                            f64::from(*x - inner.x)
                                * f64::from((v.width / u32::from(v.columns.max(1))).max(1)),
                            f64::from(*y - inner.y)
                                * f64::from((v.height / u32::from(v.rows.max(1))).max(1)),
                        ],
                        |pt| {
                            [
                                pt[0].saturating_sub(u32::from(r.x)) as f64,
                                pt[1].saturating_sub(u32::from(r.y)) as f64,
                            ]
                        },
                    );
                    p.terminal.pointer(action, *button, xy, *modifiers)
                } else {
                    Ok(())
                }
            }
            Input::CancelPointer => {
                p.terminal.release();
                Ok(())
            }
            _ => Ok(()),
        };
        let id = p.reservation.record.id.clone();
        if let Err(error) = result {
            self.note(format!("Popup input: {error:#}"));
        }
        if close {
            let _ = self.close_popup(&id);
        }
        true
    }
    fn popup_caption(&self, popup: &Popup) -> String {
        if popup.controls {
            "Enter: close · Esc: return".into()
        } else {
            self.keyboard.popup_controls_hint().map_or_else(
                || popup.reservation.record.entrypoint.title.clone(),
                |key| {
                    format!(
                        "{key}: controls · {}",
                        popup.reservation.record.entrypoint.title
                    )
                },
            )
        }
    }
    pub(super) fn popup_cells(&self, b: &mut starkit::ratatui::buffer::Buffer) {
        let Some(p) = &self.popup else {
            return;
        };
        let Ok((outer, _)) = p.reservation.record.options.bounds(self.popup_area()) else {
            return;
        };
        use starkit::ratatui::style::{Color, Style};
        let style = Style::default()
            .bg(self.appearance.bg.parse::<Color>().unwrap_or(Color::Reset))
            .fg(self.appearance.fg.parse::<Color>().unwrap_or(Color::Reset));
        for y in outer.y..outer.y + outer.height {
            for x in outer.x..outer.x + outer.width {
                b[(x, y)].set_symbol(" ").set_style(style);
            }
        }
        b.set_stringn(
            outer.x + 1,
            outer.y,
            self.popup_caption(p),
            usize::from(outer.width.saturating_sub(5)),
            style,
        );
        let close_style = if p.controls {
            style.bg(self
                .appearance
                .selection
                .parse::<Color>()
                .unwrap_or(Color::Reset))
        } else {
            style
        };
        b.set_string(outer.x + outer.width - 3, outer.y, " x ", close_style);
    }
    pub(super) fn popup_scene(&mut self, scene: &mut Scene) {
        let Some(p) = &self.popup else {
            return;
        };
        let v = scene.viewport;
        let Ok((outer, inner)) = p.reservation.record.options.bounds(self.popup_area()) else {
            return;
        };
        scene
            .components
            .retain(|c| !matches!(c, Component::Terminal { .. }));
        for component in &mut scene.components {
            if let Component::LiveCanvas { interactive, .. } = component {
                *interactive = false;
            }
        }
        // Freeze the native host plane behind the popup. Cells mode already
        // composes the underlying parsed grid into its scene buffer.
        if !self.cells && self.state.space != Space::Apps {
            let content = self.presentation_canvas();
            for (id, area) in self.visible_panes() {
                if let Some(t) = self.terminals.get(&(self.config.workspace.clone(), id)) {
                    let rect = native_terminal_rect(area, content, v).0;
                    let px = pixels(rect, v);
                    let mut surfaces = Vec::new();
                    t.paint(
                        PixelRect::new(0, 0, px.width, px.height),
                        &self.appearance,
                        &mut surfaces,
                    );
                    for surface in surfaces {
                        scene.components.push(Component::Surface { rect, surface });
                    }
                }
            }
        }
        let mut surface = Surface::new(v.width as u16, v.height as u16, "transparent".into());
        surface.fill(
            PixelRect::new(0, 0, v.width as u16, v.height as u16),
            "#00000066",
            0,
        );
        let box_px = pixels(outer, v);
        surface.fill(box_px, &self.appearance.bg, 0);
        surface.nodes.push(Primitive::Border {
            rect: box_px,
            color: self.appearance.border.clone(),
            radius: 0,
        });
        let cell_h = (v.height / u32::from(v.rows.max(1))).max(1) as u16;
        surface.nodes.push(Primitive::Text {
            rect: PixelRect::new(
                box_px.x + 4,
                box_px.y,
                box_px.width.saturating_sub(30),
                cell_h,
            ),
            text: self.popup_caption(p),
            color: self.appearance.fg.clone(),
            size: 12,
            bold: true,
            mono: true,
        });
        let close = Rect {
            x: outer.x + outer.width - 3,
            y: outer.y,
            width: 3,
            height: 1,
        };
        let close_px = pixels(close, v);
        if p.controls {
            surface.fill(close_px, &self.appearance.selection, 0);
        }
        surface.nodes.push(Primitive::Text {
            rect: close_px,
            text: " × ".into(),
            color: self.appearance.fg.clone(),
            size: 12,
            bold: true,
            mono: true,
        });
        scene.components.push(Component::Surface {
            rect: Rect {
                x: 0,
                y: 0,
                width: v.columns,
                height: v.rows,
            },
            surface,
        });
        let inner_px = pixels(inner, v);
        scene.components.push(Component::LiveCanvas {
            rect: inner,
            id: "plugin-popup".into(),
            view: ContentView::new(
                [u32::from(inner_px.width), u32::from(inner_px.height)],
                inner_px,
            )
            .unwrap(),
            // Keep one input scope while the header owns keyboard focus, so
            // keys typed immediately after the shortcut survive scene latency.
            // popup_input routes them to controls rather than the program.
            interactive: true,
        });
        scene.components.push(Component::Terminal { rect: inner });
        scene.pointer_regions = vec![close];
    }
}
