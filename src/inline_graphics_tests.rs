//! Protocol framing is tested without a PTY, filesystem image read or presenter.
#[derive(Default)]
struct Capture {
    apc: Vec<(Vec<u8>, (u16, u16))>,
    canceled: usize,
    bells: usize,
    ordering: Vec<&'static str>,
}
impl vt100::Callbacks for Capture {
    fn unhandled_apc(&mut self, screen: &mut vt100::Screen, data: &[u8]) {
        self.apc.push((data.to_vec(), screen.cursor_position()));
        self.ordering.push("apc");
    }
    fn cancelled_apc(&mut self, _: &mut vt100::Screen) {
        self.canceled += 1;
    }
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bells += 1;
    }
    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        _: Option<u8>,
        _: Option<u8>,
        _: &[&[u16]],
        c: char,
    ) {
        if c == 'c' {
            self.ordering.push("device-attributes");
        }
    }
}
#[test]
fn graphics_query_precedes_following_device_attributes() {
    let source = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c";
    for chunk in 1..=source.len() + 1 {
        let mut p = vt100::Parser::new_with_callbacks(4, 40, 0, Capture::default());
        for part in source.chunks(chunk) {
            p.process(part);
        }
        assert_eq!(p.callbacks().ordering, ["apc", "device-attributes"]);
        assert!(p.screen().contents().is_empty());
    }
}
#[test]
fn apc_boundaries_preserve_cursor_order_text_sgr_and_unicode() {
    let source = "\x1b[31mAB\x1b_Gi=7,m=1;QUJD\x1b\\text\x1b_Gm=0;RA==\x1b\\界Z";
    for chunk in 1..=source.len() + 1 {
        let mut p = vt100::Parser::new_with_callbacks(4, 40, 10, Capture::default());
        for part in source.as_bytes().chunks(chunk) {
            p.process(part);
        }
        assert_eq!(p.screen().contents(), "ABtext界Z");
        assert_eq!(
            p.screen().cell(0, 0).unwrap().fgcolor(),
            vt100::Color::Idx(1)
        );
        assert_eq!(
            p.callbacks().apc,
            vec![
                (b"Gi=7,m=1;QUJD".to_vec(), (0, 2)),
                (b"Gm=0;RA==".to_vec(), (0, 6))
            ]
        );
        assert_eq!(p.callbacks().canceled, 0);
    }
}
#[test]
fn apc_cancellation_and_overflow_never_publish_or_leak_payload() {
    let mut p = vt100::Parser::new_with_callbacks(4, 40, 10, Capture::default());
    p.process(b"\x1b]8;;https://example.invalid/\x07A");
    for cancel in [0x18, 0x1a] {
        p.process(b"\x1b_Gm=1;discard");
        p.process(&[cancel]);
    }
    p.process(b"\x1b_Gbad;discard\x1b[32mB");
    p.process(b"\x1b_G");
    for _ in 0..10 {
        p.process(&[b'x'; 1024]);
    }
    p.process(b"\x07\x1b\\C");
    assert!(p.callbacks().apc.is_empty());
    assert_eq!(p.callbacks().canceled, 4);
    assert_eq!(
        p.callbacks().bells,
        0,
        "APC BEL must not escape to the terminal bell"
    );
    assert_eq!(p.screen().contents(), "ABC");
    assert_eq!(p.screen().hyperlink(0, 2), Some("https://example.invalid/"));
    assert_eq!(
        p.screen().cell(0, 1).unwrap().fgcolor(),
        vt100::Color::Idx(2)
    );
    p.process(b"\x1b_Gi=8;QQ==\x1b\\D");
    assert_eq!(p.callbacks().apc, vec![(b"Gi=8;QQ==".to_vec(), (0, 3))]);
}
#[test]
fn ignored_strings_do_not_become_text_or_damage_following_graphics() {
    for prefix in *b"PX^" {
        let mut p = vt100::Parser::new_with_callbacks(4, 40, 10, Capture::default());
        p.process(&[0x1b, prefix]);
        p.process(b"ignored Gm=1;AAAA");
        p.process(b"\x1b");
        p.process(b"\\A\x1b_Gi=4;QQ==\x1b\\B");
        assert_eq!(p.screen().contents(), "AB");
        assert_eq!(p.callbacks().apc, vec![(b"Gi=4;QQ==".to_vec(), (0, 1))]);
    }
    let mut p = vt100::Parser::new_with_callbacks(4, 40, 10, Capture::default());
    p.process(b"\x1b_Gi=4;A\x07A\x1b\\B");
    assert_eq!(p.callbacks().apc[0].0, b"Gi=4;A\x07A");
    assert_eq!(p.callbacks().bells, 0);
    assert_eq!(p.screen().contents(), "B");
}
#[test]
fn overlong_apc_can_recover_with_a_repaint_without_a_terminator() {
    let mut p = vt100::Parser::new_with_callbacks(4, 40, 0, Capture::default());
    p.process(b"\x1b_G");
    for _ in 0..9 {
        p.process(&[b'x'; 1024]);
    }
    p.process(b"\x1b[32mRECOVERED");
    assert_eq!(p.screen().contents(), "RECOVERED");
    assert_eq!(
        p.screen().cell(0, 0).unwrap().fgcolor(),
        vt100::Color::Idx(2)
    );
    assert_eq!(p.callbacks().canceled, 1);
    assert!(p.callbacks().apc.is_empty());
}
#[test]
fn ordinary_brackets_underscores_and_capitals_do_not_confuse_the_prefilter() {
    let source = format!("{}\x1b_Gprobe;AAAA\x1b\\Z", "P^X_]_\x1b[31m".repeat(40));
    for chunk in [1, 2, 7, 16, 64, 4096] {
        let mut p = vt100::Parser::new_with_callbacks(4, 40, 10, Capture::default());
        for part in source.as_bytes().chunks(chunk) {
            p.process(part);
        }
        assert_eq!(p.callbacks().apc, vec![(b"Gprobe;AAAA".to_vec(), (3, 40))]);
        assert_eq!(p.screen().cell(3, 0).unwrap().contents(), "Z");
        assert!(!p.screen().contents().contains("probe"));
    }
}

#[test]
fn visible_snapshot_keeps_styles_links_and_cursor_without_history_or_mutation() {
    let mut parser = vt100::Parser::new(3, 20, 100);
    parser.process(b"old0\r\nold1\r\nold2\r\n\x1b[31m\x1b]8;;https://example.invalid/\x1b\\linked\x1b]8;;\x1b\\");
    let original = parser.screen().contents_formatted();
    let snapshot = parser.screen().visible_snapshot();
    assert_eq!(snapshot.contents_formatted(), original);
    assert_eq!(
        snapshot.cursor_position(),
        parser.screen().cursor_position()
    );
    assert_eq!(snapshot.hyperlink(2, 0), Some("https://example.invalid/"));
    assert_eq!(snapshot.cell(2, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
    let mut history = snapshot.clone();
    history.set_scrollback(100);
    assert_eq!(history.scrollback(), 0);
    parser.screen_mut().set_scrollback(1);
    let historical = parser.screen().visible_snapshot();
    assert_eq!(historical.contents(), parser.screen().contents());
    assert!(historical.hide_cursor());
    assert_eq!(parser.screen().scrollback(), 1);
    parser.screen_mut().set_scrollback(0);
    parser.process(b"\x1b[?1049hALTERNATE");
    let alternate = parser.screen().visible_snapshot();
    assert!(alternate.alternate_screen());
    assert_eq!(alternate.contents(), "ALTERNATE");
    assert_eq!(snapshot.contents_formatted(), original);
    parser.process(b"\x1b[?1049l");
    assert_eq!(parser.screen().contents_formatted(), original);
}
