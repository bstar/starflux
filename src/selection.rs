//! Distinguish a text-selection drag from a click without consuming app clicks.
#[derive(Clone, Debug, PartialEq)]
pub struct Pointer {
    pub action: String,
    pub button: u8,
    pub position: [f64; 2],
    pub modifiers: u8,
}
#[derive(Default)]
pub struct Gesture {
    origin: Option<Pointer>,
    selecting: bool,
}
impl Gesture {
    pub fn cancel(&mut self) {
        self.origin = None;
        self.selecting = false;
    }
    pub fn route(&mut self, event: Pointer) -> (Vec<Pointer>, bool) {
        if event.action == "down" && event.button == 0 && event.modifiers == 0 {
            self.origin = Some(event);
            self.selecting = false;
            return (vec![], false);
        }
        let Some(origin) = self.origin.clone() else {
            return (vec![event], false);
        };
        if matches!(event.action.as_str(), "move" | "drag") {
            let moved = (event.position[0] - origin.position[0])
                .hypot(event.position[1] - origin.position[1])
                >= 4.;
            if !self.selecting && !moved {
                return (vec![], false);
            }
            let mut events = vec![];
            if !self.selecting {
                let mut start = origin;
                start.modifiers |= 1;
                events.push(start);
                self.selecting = true;
            }
            let mut motion = event;
            motion.modifiers |= 1;
            events.push(motion);
            return (events, false);
        }
        if event.action == "up" && event.button == 0 {
            self.origin = None;
            let moved = (event.position[0] - origin.position[0])
                .hypot(event.position[1] - origin.position[1])
                >= 4.;
            if self.selecting || moved {
                let mut events = vec![];
                if !self.selecting {
                    let mut start = origin;
                    start.modifiers |= 1;
                    events.push(start);
                }
                self.selecting = false;
                let mut end = event.clone();
                end.modifiers |= 1;
                let mut motion = end.clone();
                motion.action = "move".into();
                let mut restore = event;
                restore.action = "move".into();
                events.extend([motion, end, restore]);
                return (events, true);
            }
            return (vec![origin, event], false);
        }
        (vec![event], false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pointer(action: &str, x: f64, modifiers: u8) -> Pointer {
        Pointer {
            action: action.into(),
            button: 0,
            position: [x, 20.],
            modifiers,
        }
    }
    #[test]
    fn click_is_forwarded_without_shift_and_small_jitter_does_not_select() {
        let mut g = Gesture::default();
        assert!(g.route(pointer("down", 10., 0)).0.is_empty());
        assert!(g.route(pointer("drag", 12., 0)).0.is_empty());
        let (events, copied) = g.route(pointer("up", 12., 0));
        assert!(!copied);
        assert_eq!(events, vec![pointer("down", 10., 0), pointer("up", 12., 0)]);
    }
    #[test]
    fn drag_selects_at_its_original_position_and_releases_synthetic_shift() {
        let mut g = Gesture::default();
        g.route(pointer("down", 10., 0));
        let (events, copied) = g.route(pointer("drag", 30., 0));
        assert!(!copied);
        assert_eq!(
            events,
            vec![pointer("down", 10., 1), pointer("drag", 30., 1)]
        );
        let (events, copied) = g.route(pointer("up", 40., 0));
        assert!(copied);
        assert_eq!(events.last().unwrap().modifiers, 0);
        assert_eq!(events[1].action, "up");
        assert_eq!(events[1].modifiers, 1);
        assert!(!g.route(pointer("up", 40., 0)).1);
    }
    #[test]
    fn a_fast_drag_still_selects_when_motion_reports_are_missing() {
        let mut gesture = Gesture::default();
        gesture.route(pointer("down", 10., 0));
        let (events, copied) = gesture.route(pointer("up", 50., 0));
        assert!(copied);
        assert_eq!(events[0], pointer("down", 10., 1));
        assert_eq!(events.last().unwrap().modifiers, 0);
    }
    #[test]
    fn modified_gestures_pass_through_and_cancel_never_copies() {
        let mut g = Gesture::default();
        let event = pointer("down", 10., 2);
        assert_eq!(g.route(event.clone()).0, vec![event]);
        g.route(pointer("down", 10., 0));
        g.route(pointer("drag", 30., 0));
        g.cancel();
        assert!(!g.route(pointer("up", 40., 0)).1);
    }
}
