//! Parser/retention tests use no live terminal or user process.
fn open(uri: &str, label: &str) -> String {
    format!("\x1b]8;id=fixture;{uri}\x1b\\{label}\x1b]8;;\x1b\\")
}
fn cells(screen: &vt100::Screen) -> Vec<(String, Option<String>, Option<String>)> {
    let (rows, cols) = screen.size();
    (0..rows)
        .flat_map(|r| {
            (0..cols).map(move |c| {
                let cell = screen.cell(r, c).unwrap();
                (
                    cell.contents().to_owned(),
                    screen.hyperlink(r, c).map(str::to_owned),
                    screen.hyperlink_params(r, c).map(str::to_owned),
                )
            })
        })
        .collect()
}
#[test]
fn osc8_streaming_bel_st_semicolons_sgr_and_unicode() {
    let uri = format!("https://example.invalid/a{}?q=界Ü#details", ";x".repeat(40));
    let mut parser = vt100::Parser::new(4, 40, 20);
    let source = format!("\x1b]8;id=test:future=value;{uri}\x07界e\u{301}\x1b[0m B\x1b]8;;\x1b\\C");
    for byte in source.as_bytes() {
        parser.process(&[*byte]);
    }
    assert_eq!(parser.screen().hyperlink(0, 0), Some(uri.as_str()));
    assert_eq!(parser.screen().hyperlink(0, 1), Some(uri.as_str()));
    assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "e\u{301}");
    assert_eq!(parser.screen().hyperlink(0, 4), Some(uri.as_str()));
    assert_eq!(parser.screen().hyperlink(0, 5), None);
    let links = crate::terminal_links::scan(parser.screen());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, uri);
    assert!(links[0].regions[0].contains(1, 0));
}
#[test]
fn osc8_repaint_erase_scroll_resize_and_alternate_screen() {
    let mut parser = vt100::Parser::new(3, 30, 10);
    parser.process(open("https://example.invalid/one", "label").as_bytes());
    let frozen = parser.screen().clone();
    let revision = parser.screen().hyperlink_revision();
    parser.process(b"\x1b[2;2H\x1b[1;1H");
    assert_eq!(
        parser.screen().hyperlink_revision(),
        revision,
        "cursor motion must not invalidate the hyperlink index"
    );
    parser.process(b"\x1b[Hlabel");
    assert!(crate::terminal_links::scan(parser.screen()).is_empty());
    assert!(parser.screen().hyperlink_revision() != revision);
    assert_eq!(
        crate::terminal_links::scan(&frozen)[0].url,
        "https://example.invalid/one"
    );
    parser.process(
        format!(
            "\r\n{}\r\nline\r\nline\r\n",
            open("sf-issue:42", "scroll target")
        )
        .as_bytes(),
    );
    parser.screen_mut().set_scrollback(10);
    assert!(
        crate::terminal_links::scan(parser.screen())
            .iter()
            .any(|l| l.url == "sf-issue:42")
    );
    parser.screen_mut().set_scrollback(0);
    parser.process(
        format!(
            "\x1b[?1049h{}",
            open("https://example.invalid/alt", "alternate")
        )
        .as_bytes(),
    );
    assert_eq!(
        crate::terminal_links::scan(parser.screen())[0].url,
        "https://example.invalid/alt"
    );
    parser.process(b"\x1b[?1049l\x1b[2J\x1b[H");
    assert!(crate::terminal_links::scan(parser.screen()).is_empty());
    parser.process(open("https://example.invalid/wide", "世界").as_bytes());
    parser.process(b"\x1b[H \x1b[2X");
    assert!(crate::terminal_links::scan(parser.screen()).is_empty());
    parser.screen_mut().set_size(1, 2);
    assert!(crate::terminal_links::scan(parser.screen()).is_empty());
}
#[test]
fn osc8_destinations_override_url_shaped_labels_and_reject_credentials() {
    let mut parser = vt100::Parser::new(5, 60, 0);
    parser.process(
        open(
            "https://example.invalid/destination",
            "https://other.invalid/label",
        )
        .as_bytes(),
    );
    let links = crate::terminal_links::scan(parser.screen());
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://example.invalid/destination");
    parser.process(
        format!(
            "\r\n{}",
            open(
                "https://user:password@example.invalid/",
                "https://safe.invalid/label"
            )
        )
        .as_bytes(),
    );
    assert_eq!(crate::terminal_links::scan(parser.screen()).len(), 1);
}
#[test]
fn osc8_full_and_diff_formatting_restore_targets_and_active_attribute() {
    let mut parser = vt100::Parser::new(5, 20, 10);
    parser.process(
        format!(
            "{} {}\r\nplain",
            open(
                "https://example.invalid/first",
                "a 界e\u{301} long wrapped label"
            ),
            open("sf-issue:42", "second")
        )
        .as_bytes(),
    );
    let original = parser.screen().clone();
    let mut restored = vt100::Parser::new(5, 20, 10);
    restored.process(&original.contents_formatted());
    assert_eq!(cells(restored.screen()), cells(&original));
    parser.process(
        format!(
            "\x1b[H{}\x1b[2;1H\x1b[2K",
            open(
                "https://example.invalid/new",
                "a 界e\u{301} long wrapped label"
            )
        )
        .as_bytes(),
    );
    restored.process(&parser.screen().contents_diff(&original));
    assert_eq!(cells(restored.screen()), cells(parser.screen()));
    parser.process(b"\x1b[H\x1b]8;;sf-issue:active\x1b\\A");
    restored.process(&parser.screen().contents_formatted());
    parser.process(b"B");
    restored.process(b"B");
    assert_eq!(cells(restored.screen()), cells(parser.screen()));
    assert_eq!(restored.screen().hyperlink(0, 1), Some("sf-issue:active"));
}
#[test]
fn osc8_resource_limits_and_cancelled_or_malformed_strings() {
    let mut parser = vt100::Parser::new(3, 20, 0);
    parser.process(b"\x1b]8;;");
    for _ in 0..400 {
        parser.process(&[b'x'; 1024]);
    }
    parser.process(b"\x1b[Hnot a terminator\x1b\\AFTER");
    assert_eq!(parser.screen().contents(), "AFTER");
    assert_eq!(parser.screen().hyperlink_bytes(), 0);
    for raw in [
        b"\x1b]8;;https://example.invalid/\x18".as_slice(),
        b"\x1b]8;;https://example.invalid/\x1b[H",
    ] {
        parser.process(b"\x1b[2J\x1b[H");
        parser.process(raw);
        parser.process(b"plain");
        assert!(crate::terminal_links::scan(parser.screen()).is_empty());
    }
    parser.process(
        open(
            &format!("https://example.invalid/{}", "x".repeat(8192)),
            "too long",
        )
        .as_bytes(),
    );
    assert_eq!(parser.screen().hyperlink_bytes(), 0);
    let mut parser = vt100::Parser::new(1, 4, 1200);
    for index in 0..1200 {
        parser.process(
            open(
                &format!("https://example.invalid/{index}/{}", "x".repeat(8000)),
                "x\r\n",
            )
            .as_bytes(),
        );
    }
    assert!(parser.screen().hyperlink_bytes() <= 8 * 1024 * 1024);
    assert!(parser.screen().hyperlink_bytes() > 7 * 1024 * 1024);
    let mut parser = vt100::Parser::new(1, 1, 0);
    for index in 0..1000 {
        parser.process(format!("\x1b[H{}", open(&format!("sf-issue:{index}"), "x")).as_bytes());
    }
    assert!(parser.screen().hyperlink_bytes() < 1024);
    parser.process(b"\x1b[2J");
    assert_eq!(parser.screen().hyperlink_bytes(), 0);
}
#[test]
fn osc8_bounded_snapshot_formatting_stops_uri_amplification() {
    let mut parser = vt100::Parser::new(20, 80, 0);
    let uri = format!("https://example.invalid/{}", "x".repeat(8000));
    parser.process(format!("\x1b]8;;{uri}\x1b\\").as_bytes());
    // One URI allocation, but many alternating linked runs on the screen.
    for column in (0..80).step_by(2) {
        for row in 1..=20 {
            parser.process(format!("\x1b[{row};{}Hx", column + 1).as_bytes());
        }
    }
    parser.process(b"\x1b]8;;\x1b\\");
    assert!(parser.screen().hyperlink_bytes() < 9000);
    assert!(
        parser
            .screen()
            .contents_formatted_bounded(1024 * 1024)
            .is_none()
    );
    let mut small = vt100::Parser::new(4, 40, 0);
    small.process(open(&uri, "reasonable label").as_bytes());
    let formatted = small
        .screen()
        .contents_formatted_bounded(1024 * 1024)
        .unwrap();
    assert!(formatted.len() < 20000);
}
#[test]
fn osc8_cold_screen_preview_preserves_hidden_destinations() {
    let config: crate::runtime::Config = serde_json::from_value(serde_json::json!({
        "workspace":"osc8-preview", "host":null,"directory":null,"agent":null,
        "executable":"starflux","project_host":true}))
    .unwrap();
    let mut parser = vt100::Parser::new(4, 40, 0);
    parser.process(
        open(
            "https://example.invalid/hidden?x=1;2#saved",
            "a label, not its URI",
        )
        .as_bytes(),
    );
    let saved = crate::cold_start::Screen::capture(
        &config,
        0,
        None,
        &serde_json::json!({}),
        parser.screen(),
    )
    .unwrap();
    let restored = saved.parse().unwrap();
    assert_eq!(cells(&restored), cells(parser.screen()));
    assert_eq!(
        crate::terminal_links::scan(&restored)[0].url,
        "https://example.invalid/hidden?x=1;2#saved"
    );
}
#[test]
fn osc8_diff_and_full_roundtrips_follow_mixed_grid_edits() {
    fn grid(screen: &vt100::Screen) -> Vec<(vt100::Cell, Option<String>, Option<String>)> {
        (0..6)
            .flat_map(|r| {
                (0..12).map(move |c| {
                    (
                        screen.cell(r, c).unwrap().clone(),
                        screen.hyperlink(r, c).map(str::to_owned),
                        screen.hyperlink_params(r, c).map(str::to_owned),
                    )
                })
            })
            .collect()
    }
    let mut parser = vt100::Parser::new(6, 12, 20);
    let mut mirror = vt100::Parser::new(6, 12, 20);
    let mut state = 7_u64;
    for index in 0..400 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let previous = parser.screen().clone();
        let row = state % 6 + 1;
        let col = (state >> 8) % 12 + 1;
        let operation = match (state >> 24) % 12 {
            0 => open(&format!("sf-issue:{index}"), "wide界 e\u{301}"),
            1 => format!("\x1b[{row};{col}Hplain"),
            2 => "\x1b[0K".into(),
            3 => "\x1b[2X".into(),
            4 => "\x1b[1P".into(),
            5 => "\x1b[2@".into(),
            6 => "\r\n".into(),
            7 => "\x1b[1S".into(),
            8 => "\x1b[1T".into(),
            9 => format!("\x1b]8;id=x;sf-issue:active{index}\x1b\\a"),
            10 => "\x1b]8;;\x1b\\\x1b[0m ".into(),
            _ => "\x1b[31;44mcolor".into(),
        };
        parser.process(operation.as_bytes());
        mirror.process(&parser.screen().contents_diff(&previous));
        for (cell, (actual, expected)) in grid(mirror.screen())
            .iter()
            .zip(grid(parser.screen()))
            .enumerate()
        {
            assert_eq!(
                actual, &expected,
                "edit {index}, cell {cell}: {operation:?}"
            );
        }
        if index % 13 == 0 {
            let mut full = vt100::Parser::new(6, 12, 0);
            full.process(&parser.screen().contents_formatted_bounded(65536).unwrap());
            for (cell, (actual, expected)) in grid(full.screen())
                .iter()
                .zip(grid(parser.screen()))
                .enumerate()
            {
                assert_eq!(actual, &expected, "full edit {index}, cell {cell}");
            }
        }
    }
}

#[test]
fn osc8_keeps_ordinary_cells_compact_and_without_drop() {
    assert_eq!(std::mem::size_of::<vt100::Cell>(), 32);
    assert!(!std::mem::needs_drop::<vt100::Cell>());
}
