// Adapted from STAR/SCREEN, MIT, Copyright (c) 2026 bstar.
use super::*;
use smithay::xwayland::{
    X11Surface,
    xwm::{Reorder, ResizeEdge, XwmHandler, XwmId},
};
impl XwmHandler for State {
    fn xwm_state(&mut self, _: XwmId) -> &mut X11Wm {
        self.xwm
            .as_mut()
            .expect("XWM callback before initialization")
    }
    fn new_window(&mut self, _: XwmId, _: X11Surface) {}
    fn new_override_redirect_window(&mut self, _: XwmId, _: X11Surface) {}
    fn map_window_request(&mut self, _: XwmId, surface: X11Surface) {
        let requested = Rectangle::new((60, 60).into(), surface.geometry().size);
        let geometry = fit_window_geometry(requested, &self.settings);
        let _ = surface.configure(geometry);
        let _ = surface.set_mapped(true);
        let w = Window::new_x11_window(surface);
        self.space.map_element(w.clone(), geometry.loc, true);
        self.restore_window(&w);
        self.focus(w);
    }
    fn mapped_override_redirect_window(&mut self, _: XwmId, surface: X11Surface) {
        let loc = surface.geometry().loc;
        self.space
            .map_element(Window::new_x11_window(surface), loc, false);
    }
    fn unmapped_window(&mut self, _: XwmId, surface: X11Surface) {
        let found_window = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(w) = found_window {
            self.space.unmap_elem(&w);
        }
    }
    fn destroyed_window(&mut self, id: XwmId, surface: X11Surface) {
        self.unmapped_window(id, surface);
    }
    fn configure_request(
        &mut self,
        _: XwmId,
        surface: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        _: Option<Reorder>,
    ) {
        let mut geo = surface.geometry();
        if let Some(x) = x {
            geo.loc.x = x;
        }
        if let Some(y) = y {
            geo.loc.y = y;
        }
        if let Some(w) = width {
            geo.size.w = w as i32;
        }
        if let Some(h) = height {
            geo.size.h = h as i32;
        }
        if !surface.is_override_redirect() {
            geo = fit_window_geometry(geo, &self.settings);
        }
        let _ = surface.configure(geo);
        let found_window = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(w) = found_window {
            self.space.map_element(w, geo.loc, false);
        }
    }
    fn configure_notify(
        &mut self,
        _: XwmId,
        surface: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _: Option<u32>,
    ) {
        // Managed positions belong to the compositor. Notifications can arrive
        // after a newer Alt-drag position has already been mapped; replaying
        // those coordinates makes the window lag or jump backwards. Unmanaged
        // popup windows still choose their own position.
        if !surface.is_override_redirect() {
            return;
        }
        let found_window = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(w) = found_window {
            self.space.map_element(w, geometry.loc, false);
        }
    }
    fn resize_request(&mut self, _: XwmId, surface: X11Surface, _: u32, _: ResizeEdge) {
        self.xdrag(surface, 10);
    }
    fn move_request(&mut self, _: XwmId, surface: X11Surface, _: u32) {
        self.xdrag(surface, 0);
    }
    fn maximize_request(&mut self, _: XwmId, surface: X11Surface) {
        self.xmaximize(surface);
    }
    fn fullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        let _ = surface.set_fullscreen(true);
        self.xmaximize(surface);
    }
    fn unfullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        let _ = surface.set_fullscreen(false);
        let _ = surface.configure(Rectangle::new((40, 40).into(), (900, 650).into()));
    }
}
impl State {
    fn xdrag(&mut self, surface: X11Surface, resize: u32) {
        let found_window = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(w) = found_window {
            self.drag = Some(Drag {
                geometry: Rectangle::new(
                    self.space.element_location(&w).unwrap_or_default(),
                    w.geometry().size,
                ),
                window: w,
                pointer: self
                    .seat
                    .get_pointer()
                    .unwrap()
                    .grab_start_data()
                    .map_or(self.seat.get_pointer().unwrap().current_location(), |g| {
                        g.location
                    }),
                resize,
            });
        }
    }
    fn xmaximize(&mut self, surface: X11Surface) {
        let found_window = self
            .space
            .elements()
            .find(|w| w.x11_surface() == Some(&surface))
            .cloned();
        if let Some(w) = found_window {
            self.size_window(
                &w,
                Rectangle::from_size(
                    (self.settings.width as i32, self.settings.height as i32).into(),
                ),
            );
        }
    }
}
