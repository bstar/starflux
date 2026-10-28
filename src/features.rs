//! Product capabilities shared by every presentation and action entrypoint.
/// A remote controller needs streamed pixels; a local one may share frames.
/// SSH itself is not a restriction. Cells frontends never display app canvases.
pub fn app_view(caps: &starkit::terminal_graphics::capabilities::Capabilities) -> bool {
    caps.cell_presentation != Some(true) && (caps.local_frames || caps.streamed_frames)
}

pub fn app_action(action: &str) -> bool {
    matches!(action, "view:apps" | "launch-app" | "run-app")
        || action.starts_with("apps:")
        || action.starts_with("app:")
        || action.starts_with("audio:")
        || action.starts_with("project-space:apps:")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn app_space_follows_each_frontends_frame_capabilities() {
        let mut caps = serde_json::from_value(serde_json::json!({
            "image_transport":"kitty", "pixel_geometry":"measured", "pointer_precision":"cells", "keyboard":true,"paste":true
        })).unwrap();
        assert!(!app_view(&caps));
        caps.streamed_frames = true;
        assert!(app_view(&caps), "SSH streaming supports App Space");
        caps.cell_presentation = Some(true);
        assert!(!app_view(&caps), "Cells cannot display app canvases");
        caps.cell_presentation = Some(false);
        caps.streamed_frames = false;
        caps.local_frames = true;
        assert!(app_view(&caps));
    }
}
