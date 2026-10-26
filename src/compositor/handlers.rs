// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
use super::*;
use smithay::wayland::seat::WaylandFocus;
use smithay::{
    backend::{allocator::dmabuf::Dmabuf, renderer::utils::on_commit_buffer_handler},
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_output,
    delegate_pointer_constraints, delegate_relative_pointer, delegate_seat, delegate_shm,
    delegate_xdg_decoration, delegate_xdg_shell, delegate_xwayland_shell,
    desktop::{PopupKind, find_popup_root_surface, get_popup_toplevel_coords},
    input::{
        SeatHandler,
        pointer::{CursorImageStatus, PointerHandle},
    },
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel,
        },
        wayland_server::{
            Client,
            protocol::{wl_buffer::WlBuffer, wl_seat::WlSeat},
        },
    },
    utils::Serial,
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorHandler, get_parent, is_sync_subsurface, with_states},
        dmabuf::{DmabufGlobal, DmabufHandler, ImportNotifier},
        output::OutputHandler,
        pointer_constraints::PointerConstraintsHandler,
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, ServerDndGrabHandler,
                set_data_device_focus,
            },
        },
        shell::xdg::{
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler,
            XdgToplevelSurfaceData, decoration::XdgDecorationHandler,
        },
        shm::ShmHandler,
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::XWaylandClientData,
};

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }
    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            &data.compositor
        } else {
            &client
                .get_data::<XWaylandClientData>()
                .unwrap()
                .compositor_state
        }
    }
    fn commit(&mut self, surface: &WlSurface) {
        self.clock.pending = true;
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            let found_window = self
                .space
                .elements()
                .find(|w| w.wl_surface().as_deref() == Some(&root))
                .cloned();
            if let Some(w) = found_window {
                w.on_commit();
                if let Some(t) = w.toplevel() {
                    let sent = with_states(surface, |s| {
                        s.data_map
                            .get::<XdgToplevelSurfaceData>()
                            .map(|d| d.lock().unwrap().initial_configure_sent)
                            .unwrap_or(true)
                    });
                    if !sent {
                        t.send_configure();
                    }
                }
            }
        }
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(p)) = self.popups.find_popup(surface)
            && !p.is_initial_configure_sent()
        {
            let _ = p.send_configure();
        }
    }
}
impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _: &WlBuffer) {}
}
impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}
impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;
    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
    fn cursor_image(&mut self, _: &Seat<Self>, _: CursorImageStatus) {}
    fn focus_changed(&mut self, seat: &Seat<Self>, focus: Option<&WlSurface>) {
        set_data_device_focus(
            &self.dh,
            seat,
            focus.and_then(|s| self.dh.get_client(s.id()).ok()),
        );
    }
}
impl SelectionHandler for State {
    type SelectionUserData = std::sync::Arc<String>;
    fn send_selection(
        &mut self,
        _: smithay::wayland::selection::SelectionTarget,
        _: String,
        fd: std::os::fd::OwnedFd,
        _: Seat<Self>,
        data: &Self::SelectionUserData,
    ) {
        let data = data.clone();
        std::thread::spawn(move || {
            use std::io::Write;
            let mut file = std::fs::File::from(fd);
            let _ = file.write_all(data.as_bytes());
        });
    }
}
impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}
impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}
impl OutputHandler for State {}
impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf
    }
    fn dmabuf_imported(&mut self, _: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        if self.renderer.import_dmabuf(&dmabuf, None).is_ok() {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
    }
}
impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if pointer
            .current_focus()
            .as_ref()
            .is_some_and(|s| s == surface)
        {
            // Absolute terminal pointer input cannot safely activate a relative lock.
            self.locked = true;
            let _ = self.events.send(DesktopEvent::Locked(true));
        }
    }
    fn cursor_position_hint(
        &mut self,
        _: &WlSurface,
        _: &PointerHandle<Self>,
        _: Point<f64, Logical>,
    ) {
    }
}
impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }
}
impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, t: ToplevelSurface) {
        // SCREEN supplies a borderless desktop. Suppress client window chrome
        // for applications that negotiate decorations; Alt-drag moves windows.
        t.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        t.send_pending_configure();
    }
    fn request_mode(&mut self, t: ToplevelSurface, _: DecorationMode) {
        self.new_decoration(t);
    }
    fn unset_mode(&mut self, t: ToplevelSurface) {
        self.new_decoration(t);
    }
}
impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.shell
    }
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let w = Window::new_wayland_window(surface);
        let count = self.space.elements().count() as i32;
        let available = (
            (self.settings.width as i32 - 100).max(320),
            (self.settings.height as i32 - 100).max(240),
        );
        w.toplevel().unwrap().with_pending_state(|s| {
            s.size = Some((available.0.min(1100), available.1.min(760)).into())
        });
        self.space
            .map_element(w.clone(), (40 + count * 28, 40 + count * 28), true);
        self.restore_window(&w);
        self.focus(w);
    }
    fn new_popup(&mut self, surface: PopupSurface, _: PositionerState) {
        self.unconstrain(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }
    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|s| {
            s.geometry = positioner.get_geometry();
            s.positioner = positioner;
        });
        self.unconstrain(&surface);
        surface.send_repositioned(token);
    }
    fn grab(&mut self, _: PopupSurface, _: WlSeat, _: Serial) {}
    fn move_request(&mut self, surface: ToplevelSurface, _: WlSeat, serial: Serial) {
        self.begin_drag(surface.wl_surface(), serial, 0);
    }
    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _: WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        self.begin_drag(surface.wl_surface(), serial, edges as u32);
    }
    fn maximize_request(&mut self, t: ToplevelSurface) {
        t.with_pending_state(|s| {
            s.states.set(xdg_toplevel::State::Maximized);
            s.size = Some((self.settings.width as i32, self.settings.height as i32).into());
        });
        let found_window = self
            .space
            .elements()
            .find(|w| w.toplevel() == Some(&t))
            .cloned();
        if let Some(w) = found_window {
            self.space.map_element(w, (0, 0), true);
        }
        t.send_pending_configure();
    }
    fn unmaximize_request(&mut self, t: ToplevelSurface) {
        t.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Maximized);
            s.size = Some(
                (
                    900.min(self.settings.width as i32),
                    650.min(self.settings.height as i32),
                )
                    .into(),
            );
        });
        t.send_pending_configure();
    }
    fn fullscreen_request(
        &mut self,
        t: ToplevelSurface,
        _: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        t.with_pending_state(|s| {
            s.states.set(xdg_toplevel::State::Fullscreen);
            s.size = Some((self.settings.width as i32, self.settings.height as i32).into());
        });
        let found_window = self
            .space
            .elements()
            .find(|w| w.toplevel() == Some(&t))
            .cloned();
        if let Some(w) = found_window {
            self.space.map_element(w, (0, 0), true);
        }
        t.send_pending_configure();
    }
    fn unfullscreen_request(&mut self, t: ToplevelSurface) {
        t.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Fullscreen);
            s.size = Some(
                (
                    900.min(self.settings.width as i32),
                    650.min(self.settings.height as i32),
                )
                    .into(),
            );
        });
        t.send_pending_configure();
    }
}
impl State {
    fn begin_drag(&mut self, surface: &WlSurface, serial: Serial, resize: u32) {
        let pointer = self.seat.get_pointer().unwrap();
        tracing::debug!(
            valid_grab = pointer.has_grab(serial),
            resize,
            "Demo window drag request"
        );
        if !pointer.has_grab(serial) {
            return;
        }
        let found_window = self
            .space
            .elements()
            .find(|w| w.wl_surface().as_deref() == Some(surface))
            .cloned();
        tracing::debug!(found = found_window.is_some(), "Demo drag target");
        if let Some(w) = found_window {
            self.drag = Some(Drag {
                geometry: Rectangle::new(
                    self.space.element_location(&w).unwrap_or_default(),
                    w.geometry().size,
                ),
                window: w,
                pointer: pointer
                    .grab_start_data()
                    .map_or(pointer.current_location(), |g| g.location),
                resize,
            });
            // The client may request the move after one or more coalesced
            // motions. Catch up from the original press without waiting again.
            let location = pointer.current_location();
            self.input(super::Input::Motion {
                x: location.x,
                y: location.y,
            });
        }
    }
    fn unconstrain(&self, p: &PopupSurface) {
        let kind = PopupKind::Xdg(p.clone());
        if let Ok(root) = find_popup_root_surface(&kind)
            && let Some(w) = self
                .space
                .elements()
                .find(|w| w.wl_surface().as_deref() == Some(&root))
        {
            let mut target = Rectangle::from_size(
                (self.settings.width as i32, self.settings.height as i32).into(),
            );
            target.loc -= get_popup_toplevel_coords(&kind);
            target.loc -= self.space.element_location(w).unwrap_or_default();
            p.with_pending_state(|s| s.geometry = s.positioner.get_unconstrained_geometry(target));
        }
    }
}
delegate_compositor!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_output!(State);
delegate_data_device!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_dmabuf!(State);
delegate_relative_pointer!(State);
delegate_pointer_constraints!(State);
delegate_xwayland_shell!(State);
