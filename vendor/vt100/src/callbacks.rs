/// The terminal operation responsible for a graphics-visible clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenClear {
    /// Erase the current visible display, preserving history and stored assets.
    Display,
    /// Enter or clear the alternate buffer with private mode 1049.
    Alternate,
    /// Reset both terminal buffers and their retained state.
    Reset,
}
/// This trait is used by the parser to handle extra escape sequences that
/// don't have an impact on the terminal screen directly.
pub trait Callbacks {
    /// Enable source-owned scroll notifications. The default leaves the normal
    /// parser's character path free of scroll-event delivery work.
    const TRACK_SCROLL: bool = false;
    /// Rows moved in a half-open page interval. Negative values scroll up.
    /// Delivered after text mutation, including automatic wrapping and LF.
    fn scrolled(&mut self, _: &mut crate::Screen, _top: u16, _bottom: u16, _rows: i32) {}
    /// A full display erase, alternate-buffer clear, or terminal reset.
    fn screen_cleared(&mut self, _: &mut crate::Screen, _kind: ScreenClear) {}
    /// This callback is called when the terminal requests an audible bell
    /// (typically with `^G`).
    fn audible_bell(&mut self, _: &mut crate::Screen) {}
    /// This callback is called when the terminal requests a visual bell
    /// (typically with `\eg`).
    fn visual_bell(&mut self, _: &mut crate::Screen) {}
    /// This callback is called when the terminal requests a resize
    /// (typically with `\e[8;<rows>;<cols>t`).
    fn resize(&mut self, _: &mut crate::Screen, _request: (u16, u16)) {}
    /// This callback is called when the terminal requests the window title
    /// to be set (typically with `\e]1;<icon_name>\a`)
    fn set_window_icon_name(&mut self, _: &mut crate::Screen, _icon_name: &[u8]) {}
    /// This callback is called when the terminal requests the window title
    /// to be set (typically with `\e]2;<title>\a`)
    fn set_window_title(&mut self, _: &mut crate::Screen, _title: &[u8]) {}
    /// This callback is called when the terminal requests data to be copied
    /// to the system clipboard (typically with `\e]52;<ty>;<data>\a`). Note
    /// that `data` will be encoded as base64.
    fn copy_to_clipboard(&mut self, _: &mut crate::Screen, _ty: &[u8], _data: &[u8]) {}
    /// This callback is called when the terminal requests data to be pasted
    /// from the system clipboard (typically with `\e]52;<ty>;?\a`).
    fn paste_from_clipboard(&mut self, _: &mut crate::Screen, _ty: &[u8]) {}
    /// This callback is called when the terminal receives an escape sequence
    /// which is otherwise not implemented.
    fn unhandled_char(&mut self, _: &mut crate::Screen, _c: char) {}
    /// This callback is called when the terminal receives a control
    /// character which is otherwise not implemented.
    fn unhandled_control(&mut self, _: &mut crate::Screen, _b: u8) {}
    /// This callback is called when the terminal receives an escape sequence
    /// which is otherwise not implemented.
    fn unhandled_escape(
        &mut self,
        _: &mut crate::Screen,
        _i1: Option<u8>,
        _i2: Option<u8>,
        _b: u8,
    ) {
    }
    /// This callback is called when the terminal receives a CSI sequence
    /// (`\e[`) which is otherwise not implemented.
    fn unhandled_csi(
        &mut self,
        _: &mut crate::Screen,
        _i1: Option<u8>,
        _i2: Option<u8>,
        _params: &[&[u16]],
        _c: char,
    ) {
    }
    /// This callback is called when the terminal receives a OSC sequence
    /// (`\e]`) which is otherwise not implemented.
    fn unhandled_osc(&mut self, _: &mut crate::Screen, _params: &[&[u8]]) {}
    /// A complete 7-bit APC string, without its introducer or ST terminator.
    /// Bodies are bounded to 8192 bytes; canceled/oversized strings are ignored.
    /// The screen reflects all preceding bytes at the instant ST was received.
    fn unhandled_apc(&mut self, _: &mut crate::Screen, _data: &[u8]) {}
    /// An APC was canceled, malformed or exceeded its size limit. Consumers
    /// should discard any pending chunked transmission, without displaying it.
    fn cancelled_apc(&mut self, _: &mut crate::Screen) {}
}

impl Callbacks for () {}
