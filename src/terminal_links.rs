//! Visible URL text only; OSC 8 destinations require parser-level attributes.
use starkit::terminal_graphics::protocol::Rect;

#[derive(Clone, Debug)]
pub struct Link {
    pub url: String,
    pub regions: Vec<Rect>,
}
/// Pointer shape needs the union of linked cells, independent of destination.
/// Merge both axes so repeated labels do not inflate every scene/hover scan.
pub fn pointer_regions(area: Rect, links: &[Link]) -> Vec<Rect> {
    let mut rows = links
        .iter()
        .flat_map(|l| l.regions.iter())
        .filter(|r| r.x < area.width && r.y < area.height)
        .map(|r| Rect {
            x: r.x,
            y: r.y,
            width: r.width.min(area.width - r.x),
            height: 1,
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|r| (r.y, r.x));
    let mut horizontal: Vec<Rect> = Vec::new();
    for rect in rows {
        if let Some(previous) = horizontal
            .last_mut()
            .filter(|r| r.y == rect.y && rect.x <= r.x + r.width)
        {
            previous.width = previous.width.max(rect.x + rect.width - previous.x);
        } else {
            horizontal.push(rect);
        }
    }
    let mut result: Vec<Rect> = Vec::new();
    let mut columns = std::collections::BTreeMap::<(u16, u16), usize>::new();
    for rect in horizontal {
        let key = (rect.x, rect.width);
        if let Some(previous) = columns
            .get(&key)
            .and_then(|i| result.get_mut(*i))
            .filter(|r| r.y + r.height == rect.y)
        {
            previous.height += 1;
        } else {
            columns.insert(key, result.len());
            result.push(rect);
        }
    }
    for rect in &mut result {
        rect.x += area.x;
        rect.y += area.y;
    }
    result
}
struct Cell {
    start: usize,
    end: usize,
    row: u16,
    column: u16,
    width: u16,
    explicit: bool,
}
fn delimiter(c: char) -> bool {
    c.is_whitespace() || c.is_control() || "<>\"'`“”‘’".contains(c)
}
fn scheme_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"+.-".contains(&b)
}
fn collect(text: &str, cells: &[Cell], cut: bool, links: &mut Vec<Link>) {
    let mut from = 0;
    while links.len() < 512 {
        let Some(colon) = text[from..].find(':').map(|n| from + n) else {
            break;
        };
        from = colon + 1;
        let mut start = colon;
        while start > 0 && scheme_byte(text.as_bytes()[start - 1]) {
            start -= 1;
        }
        if start == colon
            || !text.as_bytes()[start].is_ascii_alphabetic()
            || (start > 0
                && (text.as_bytes()[start - 1].is_ascii_alphanumeric()
                    || text.as_bytes()[start - 1] == b'_'))
            || text[from..].starts_with(':')
        {
            continue;
        }
        let mut end = text[from..]
            .char_indices()
            .find(|(_, c)| delimiter(*c))
            .map_or(text.len(), |(n, _)| from + n);
        if cut && end == text.len() {
            continue;
        }
        while end > from {
            let tail = text[..end].chars().next_back().unwrap();
            let unmatched = match tail {
                ')' => {
                    text[start..end].matches(')').count() > text[start..end].matches('(').count()
                }
                ']' => {
                    text[start..end].matches(']').count() > text[start..end].matches('[').count()
                }
                '}' => {
                    text[start..end].matches('}').count() > text[start..end].matches('{').count()
                }
                _ => false,
            };
            if ".,;!".contains(tail) || unmatched {
                end -= tail.len_utf8();
            } else {
                break;
            }
        }
        if end <= from || end - start > 8192 {
            continue;
        }
        let url = &text[start..end];
        if cells
            .iter()
            .any(|c| c.explicit && c.start < end && c.end > start)
        {
            from = end;
            continue;
        }
        if crate::plugin_links::parse(url).is_err() {
            from = end;
            continue;
        }
        let mut regions: Vec<Rect> = Vec::new();
        for cell in cells.iter().filter(|c| c.start < end && c.end > start) {
            if let Some(last) = regions.last_mut()
                && last.y == cell.row
                && last.x + last.width == cell.column
            {
                last.width += cell.width;
            } else {
                regions.push(Rect {
                    x: cell.column,
                    y: cell.row,
                    width: cell.width,
                    height: 1,
                });
            }
        }
        links.push(Link {
            url: url.into(),
            regions,
        });
        from = end;
    }
}
pub fn scan(screen: &vt100::Screen) -> Vec<Link> {
    let (rows, columns) = screen.size();
    let mut links: Vec<Link> = Vec::new();
    let mut plain = Vec::new();
    let mut explicit = std::collections::BTreeMap::<&str, usize>::new();
    // Strings are immutable shared allocations. Cache by address for this
    // borrow only, avoiding repeated URI parsing/comparison on every cell.
    let mut identities = std::collections::HashMap::<usize, Option<usize>>::new();
    let mut text = String::new();
    let mut cells = Vec::new();
    let mut oversized = false;
    for row in 0..rows {
        for column in 0..columns {
            let Some(cell) = screen
                .cell(row, column)
                .filter(|c| !c.is_wide_continuation())
            else {
                continue;
            };
            let width = if cell.is_wide() { 2 } else { 1 };
            if let Some(uri) = screen.hyperlink(row, column) {
                let index = *identities.entry(uri.as_ptr() as usize).or_insert_with(|| {
                    if let Some(index) = explicit.get(uri) {
                        return Some(*index);
                    }
                    if links.len() >= 512 || crate::plugin_links::parse(uri).is_err() {
                        return None;
                    }
                    let index = links.len();
                    links.push(Link {
                        url: uri.into(),
                        regions: Vec::new(),
                    });
                    explicit.insert(uri, index);
                    Some(index)
                });
                if let Some(index) = index {
                    if let Some(last) = links[index]
                        .regions
                        .last_mut()
                        .filter(|r| r.y == row && r.x + r.width == column)
                    {
                        last.width += width;
                    } else {
                        links[index].regions.push(Rect {
                            x: column,
                            y: row,
                            width,
                            height: 1,
                        });
                    }
                }
            }
            if oversized {
                continue;
            }
            let start = text.len();
            text.push_str(if cell.has_contents() {
                cell.contents()
            } else {
                " "
            });
            if text.len() > 8192 {
                oversized = true;
                continue;
            }
            cells.push(Cell {
                start,
                end: text.len(),
                row,
                column,
                width,
                explicit: screen.hyperlink(row, column).is_some(),
            });
        }
        let wrapped = screen.row_wrapped(row);
        if !wrapped || row + 1 == rows {
            if !oversized {
                collect(&text, &cells, wrapped, &mut plain);
            }
            text.clear();
            cells.clear();
            oversized = false;
        }
    }
    let remaining = 512 - links.len();
    links.extend(plain.into_iter().take(remaining));
    links
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_region_union_preserves_cells_and_merges_repeated_runs() {
        let mut links = vec![
            Link {
                url: "sf-issue:even".into(),
                regions: Vec::new(),
            },
            Link {
                url: "sf-issue:odd".into(),
                regions: Vec::new(),
            },
        ];
        for y in 0..12 {
            for x in 0..32 {
                links[usize::from(x % 2)].regions.push(Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                });
            }
        }
        let area = Rect {
            x: 7,
            y: 9,
            width: 32,
            height: 12,
        };
        assert_eq!(pointer_regions(area, &links), vec![area]);
        let stripes = pointer_regions(area, &links[..1]);
        assert_eq!(stripes.len(), 16);
        for y in 0..12 {
            for x in 0..32 {
                assert_eq!(stripes.iter().any(|r| r.contains(x + 7, y + 9)), x % 2 == 0);
            }
        }
        let clipped = pointer_regions(
            Rect {
                width: 17,
                height: 5,
                ..area
            },
            &links[..1],
        );
        assert_eq!(clipped.len(), 9);
        assert!(
            clipped
                .iter()
                .all(|r| r.height == 5 && r.x < 24 && r.y == 9)
        );
    }
    #[test]
    fn wrapped_unicode_links_map_to_real_cells_and_exclude_padding() {
        let mut parser = vt100::Parser::new(8, 32, 20);
        let url = "https://example.invalid/界?q=12#part";
        parser.process(format!("世界 ({url}).\r\nhttps://other.invalid/next\r\nnot_a_scheme:no\r\nhttps://user:pass@example.invalid/\r\n").as_bytes());
        let links = scan(parser.screen());
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, url);
        assert_eq!(links[0].regions[0].x, 6);
        assert_eq!(links[0].regions.len(), 2);
        assert!(links[0].regions[0].contains(6, 0));
        assert!(!links[0].regions[0].contains(4, 0));
        assert_eq!(links[1].url, "https://other.invalid/next");
        parser.process(b"\x1b[2J\x1b[Hno links");
        assert!(scan(parser.screen()).is_empty());
    }
    #[test]
    fn scrollback_alternate_screen_and_overlong_lines_are_bounded() {
        let mut parser = vt100::Parser::new(3, 40, 20);
        parser.process(b"https://example.invalid/old\r\nline2\r\nline3\r\nline4\r\n");
        assert!(scan(parser.screen()).is_empty());
        parser.screen_mut().set_scrollback(20);
        assert!(
            scan(parser.screen())
                .iter()
                .any(|l| l.url.ends_with("/old"))
        );
        parser.screen_mut().set_scrollback(0);
        parser.process(b"\x1b[?1049hhttps://example.invalid/alternate");
        assert!(
            scan(parser.screen())
                .iter()
                .any(|l| l.url.ends_with("/alternate"))
        );
        parser.process(b"\x1b[?1049l");
        assert!(scan(parser.screen()).is_empty());
        let mut parser = vt100::Parser::new(192, 100, 0);
        parser.process(format!("https://example.invalid/{}", "x".repeat(18000)).as_bytes());
        assert!(scan(parser.screen()).is_empty());
    }
}
