//! Frontend-local, frozen terminal review. No review key is sent to the PTY.
use serde::Serialize;
type Position = (usize, u16);
#[derive(Clone)]
struct Span {
    bytes: std::ops::Range<usize>,
    position: Position,
    end: Position,
}
pub struct History {
    pub screen: vt100::Screen,
    maximum: usize,
    cursor: Position,
    anchor: Option<Position>,
    text: String,
    spans: Vec<Span>,
    rows: Vec<std::ops::Range<usize>>,
    query: String,
    editing: Option<String>,
    edit_cursor: usize,
    pending_pages: i32,
    matches: Vec<(Position, Position)>,
    matched: Option<usize>,
    wrapped: bool,
    pub loading: bool,
    pub source: String,
}
#[derive(Serialize)]
pub struct Status<'a> {
    pub query: &'a str,
    pub editing: bool,
    pub query_cursor: Option<usize>,
    pub matches: usize,
    pub matched: Option<usize>,
    pub cursor: Position,
    pub selecting: bool,
    pub offset: usize,
    pub retained_rows: usize,
    pub wrapped: bool,
    pub loading: bool,
    pub source: &'a str,
}
pub enum Outcome {
    Stay,
    Exit,
    Copy(String),
}
impl History {
    pub fn new(source: &vt100::Screen, search: bool) -> Self {
        let mut screen = source.clone();
        let original = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let maximum = screen.scrollback();
        let (rows, cols) = screen.size();
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut document_rows = Vec::new();
        for row in 0..maximum + usize::from(rows) {
            screen.set_scrollback(maximum.saturating_sub(row));
            let local = row.saturating_sub(maximum) as u16;
            let start = text.len();
            for col in 0..cols {
                let Some(cell) = screen.cell(local, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let begin = text.len();
                text.push_str(if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                });
                spans.push(Span {
                    bytes: begin..text.len(),
                    position: (row, col),
                    end: (row, (col + u16::from(cell.is_wide())).min(cols - 1)),
                });
            }
            if !screen.row_wrapped(local) {
                let end = start + text[start..].trim_end_matches(' ').len();
                text.truncate(end);
                while spans.last().is_some_and(|s| s.bytes.start >= end) {
                    spans.pop();
                }
                document_rows.push(start..text.len());
                text.push('\n');
            } else {
                document_rows.push(start..text.len());
            }
        }
        screen.set_scrollback(original);
        Self {
            screen,
            maximum,
            cursor: (maximum - original, 0),
            anchor: None,
            text,
            spans,
            rows: document_rows,
            query: String::new(),
            editing: search.then(String::new),
            edit_cursor: 0,
            pending_pages: 0,
            matches: Vec::new(),
            matched: None,
            wrapped: false,
            loading: false,
            source: "attached screen".into(),
        }
    }
    fn top(&self) -> usize {
        self.maximum - self.screen.scrollback()
    }
    fn last(&self) -> usize {
        self.maximum + usize::from(self.screen.size().0) - 1
    }
    fn reveal(&mut self) {
        let rows = usize::from(self.screen.size().0);
        let top = self.top();
        let top = if self.cursor.0 < top {
            self.cursor.0
        } else if self.cursor.0 >= top + rows {
            self.cursor.0 + 1 - rows
        } else {
            top
        };
        self.screen.set_scrollback(self.maximum.saturating_sub(top));
    }
    pub fn status(&self) -> Status<'_> {
        Status {
            query: self.editing.as_deref().unwrap_or(&self.query),
            editing: self.editing.is_some(),
            query_cursor: self.editing.as_ref().map(|_| self.edit_cursor),
            matches: self.matches.len(),
            matched: self.matched.map(|n| n + 1),
            cursor: self.cursor,
            selecting: self.anchor.is_some(),
            offset: self.screen.scrollback(),
            retained_rows: self.last() + 1,
            wrapped: self.wrapped,
            loading: self.loading,
            source: &self.source,
        }
    }
    pub fn hint(&self) -> String {
        if self.loading {
            return "Loading retained terminal history · type search query · Esc: cancel".into();
        }
        if let Some(query) = &self.editing {
            let left = query[..self.edit_cursor]
                .chars()
                .rev()
                .take(48)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>();
            let right = query[self.edit_cursor..]
                .chars()
                .take(24)
                .collect::<String>();
            return format!("Search history: {left}│{right} · Enter: search · Esc: cancel");
        }
        format!(
            "Copy mode ({}) · row {}/{} · /: search · {}{} · arrows/PgUp/PgDn · Space: select · Enter/y: copy · Esc: exit",
            self.source,
            self.cursor.0 + 1,
            self.last() + 1,
            if self.query.is_empty() {
                "n/N: matches".into()
            } else {
                format!(
                    "{}: {}/{}",
                    self.query,
                    self.matched.map_or(0, |n| n + 1),
                    self.matches.len()
                )
            },
            if self.wrapped { " (wrapped)" } else { "" }
        )
    }
    pub fn replace_snapshot(&mut self, screen: &vt100::Screen, source: &str) {
        let mut next = Self::new(screen, false);
        next.query = std::mem::take(&mut self.query);
        next.editing = self.editing.take();
        next.edit_cursor = self.edit_cursor;
        next.source = source.into();
        next.find();
        if self.pending_pages != 0 {
            let page = i64::from(next.screen.size().0.saturating_sub(1).max(1));
            next.cursor.0 = (next.cursor.0 as i64 + i64::from(self.pending_pages) * page)
                .clamp(0, next.last() as i64) as usize;
            next.reveal();
        }
        *self = next;
    }
    pub fn selected(&self, row: u16, col: u16) -> bool {
        let p = (self.top() + usize::from(row), col);
        let range = self
            .anchor
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
            .or_else(|| self.matched.map(|n| self.matches[n]));
        p == self.cursor || range.is_some_and(|(a, b)| p >= a && p <= b)
    }
    fn selected_text(&self) -> Option<String> {
        let (a, b) = self
            .anchor
            .map(|a| (a.min(self.cursor), a.max(self.cursor)))
            .or_else(|| self.matched.map(|n| self.matches[n]))?;
        let start = self.spans.partition_point(|s| s.end < a);
        let end = self.spans.partition_point(|s| s.position <= b);
        let first = self.spans.get(start);
        let last = end.checked_sub(1).and_then(|n| self.spans.get(n));
        let start = first
            .filter(|s| s.position.0 == a.0)
            .map_or(self.rows[a.0].end, |s| s.bytes.start);
        let end = last
            .filter(|s| s.position.0 == b.0)
            .map_or(self.rows[b.0].start, |s| s.bytes.end);
        (start < end).then(|| self.text[start..end].chars().take(65536).collect())
    }
    fn find(&mut self) {
        self.matches.clear();
        self.matched = None;
        self.anchor = None;
        self.wrapped = false;
        if self.query.is_empty() {
            return;
        }
        for (start, _) in self.text.match_indices(&self.query) {
            let end = start + self.query.len();
            let first = self.spans.partition_point(|s| s.bytes.end <= start);
            let last = self.spans.partition_point(|s| s.bytes.start < end);
            if let (Some(a), Some(b)) = (
                self.spans.get(first),
                last.checked_sub(1).and_then(|n| self.spans.get(n)),
            ) {
                self.matches.push((a.position, b.end));
            }
        }
        if let Some(n) = self
            .matches
            .iter()
            .rposition(|(a, _)| *a <= self.cursor)
            .or_else(|| self.matches.len().checked_sub(1))
        {
            self.matched = Some(n);
            self.cursor = self.matches[n].0;
            self.reveal();
        }
    }
    fn next(&mut self, backward: bool) {
        if self.matches.is_empty() {
            return;
        }
        let current = self.matched.unwrap_or(0);
        self.wrapped = if backward {
            current == 0
        } else {
            current + 1 == self.matches.len()
        };
        let n = if backward {
            (current + self.matches.len() - 1) % self.matches.len()
        } else {
            (current + 1) % self.matches.len()
        };
        self.matched = Some(n);
        self.anchor = None;
        self.cursor = self.matches[n].0;
        self.reveal();
    }
    pub fn text_input(&mut self, text: &str) {
        if let Some(query) = &mut self.editing {
            for c in text.chars().filter(|c| !c.is_control()) {
                if query.len() + c.len_utf8() > 4096 {
                    break;
                }
                query.insert(self.edit_cursor, c);
                self.edit_cursor += c.len_utf8();
            }
        }
    }
    pub fn key(&mut self, code: &str, modifiers: u8) -> Outcome {
        if code.eq_ignore_ascii_case("char:c") && modifiers & 2 != 0 {
            return if modifiers & 1 != 0 {
                self.selected_text().map_or(Outcome::Stay, Outcome::Copy)
            } else {
                Outcome::Exit
            };
        }
        if self.editing.is_some() {
            match code {
                "escape" => {
                    self.editing = None;
                }
                "enter" => {
                    self.query = self.editing.take().unwrap();
                    self.find();
                }
                "backspace" => {
                    if let Some((n, _)) = self.editing.as_ref().unwrap()[..self.edit_cursor]
                        .char_indices()
                        .next_back()
                    {
                        self.editing.as_mut().unwrap().drain(n..self.edit_cursor);
                        self.edit_cursor = n;
                    }
                }
                "delete" => {
                    let query = self.editing.as_mut().unwrap();
                    if let Some(c) = query[self.edit_cursor..].chars().next() {
                        query.drain(self.edit_cursor..self.edit_cursor + c.len_utf8());
                    }
                }
                "left" => {
                    self.edit_cursor = self.editing.as_ref().unwrap()[..self.edit_cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(n, _)| n)
                }
                "right" => {
                    self.edit_cursor += self.editing.as_ref().unwrap()[self.edit_cursor..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
                }
                "home" => self.edit_cursor = 0,
                "end" => self.edit_cursor = self.editing.as_ref().unwrap().len(),
                "char:u" if modifiers & 2 != 0 => {
                    self.editing.as_mut().unwrap().clear();
                    self.edit_cursor = 0;
                }
                "space" => self.text_input(" "),
                _ if modifiers & 6 == 0 && code.starts_with("char:") => self.text_input(
                    &String::from_utf8_lossy(&crate::terminal::key_bytes(code, modifiers, false)),
                ),
                _ => {}
            }
            return Outcome::Stay;
        }
        if self.loading && matches!(code, "pageup" | "pagedown") {
            self.pending_pages =
                (self.pending_pages + if code == "pageup" { -1 } else { 1 }).clamp(-4000, 4000);
            return Outcome::Stay;
        }
        if self.loading && !matches!(code, "escape" | "char:q" | "char:/") {
            return Outcome::Stay;
        }
        let rows = usize::from(self.screen.size().0.saturating_sub(1).max(1));
        let cols = self.screen.size().1;
        match code {
            "escape" | "char:q" => return Outcome::Exit,
            "enter" | "char:y" => return self.selected_text().map_or(Outcome::Stay, Outcome::Copy),
            "char:/" => {
                self.editing = Some(self.query.clone());
                self.edit_cursor = self.query.len();
            }
            "char:n" | "char:N" => self.next(code == "char:N" || modifiers & 1 != 0),
            "space" | "char: " | "char:v" => {
                self.anchor = if self.anchor.is_some() {
                    None
                } else {
                    Some(self.cursor)
                };
                self.matched = None;
            }
            "up" | "char:k" => self.cursor.0 = self.cursor.0.saturating_sub(1),
            "down" | "char:j" => self.cursor.0 = (self.cursor.0 + 1).min(self.last()),
            "left" | "char:h" => self.cursor.1 = self.cursor.1.saturating_sub(1),
            "right" | "char:l" => self.cursor.1 = (self.cursor.1 + 1).min(cols - 1),
            "pageup" => self.cursor.0 = self.cursor.0.saturating_sub(rows),
            "pagedown" => self.cursor.0 = (self.cursor.0 + rows).min(self.last()),
            "home" if modifiers & 2 != 0 => self.cursor = (0, 0),
            "end" if modifiers & 2 != 0 => self.cursor = (self.last(), cols - 1),
            "home" => self.cursor.1 = 0,
            "end" => self.cursor.1 = cols - 1,
            "char:g" if modifiers & 1 == 0 => self.cursor = (0, 0),
            "char:g" | "char:G" => self.cursor = (self.last(), cols - 1),
            _ => {}
        }
        self.reveal();
        Outcome::Stay
    }
    pub fn pointer(&mut self, action: &str, button: u8, row: u16, col: u16) -> Option<String> {
        if self.loading {
            return None;
        }
        if action.starts_with("scroll_") {
            let top = if action == "scroll_up" {
                self.top().saturating_sub(3)
            } else {
                (self.top() + 3).min(self.maximum)
            };
            self.cursor.0 =
                top + (self.cursor.0 - self.top()).min(usize::from(self.screen.size().0 - 1));
            self.screen.set_scrollback(self.maximum - top);
        } else if button == 0 && matches!(action, "down" | "move" | "up") {
            let p = (
                self.top() + usize::from(row.min(self.screen.size().0 - 1)),
                col.min(self.screen.size().1 - 1),
            );
            if action == "down" {
                self.cursor = p;
                self.anchor = Some(p);
                self.matched = None;
            } else if self.anchor.is_some() {
                self.cursor = p;
            }
            if action == "up" {
                return self.selected_text();
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_search_unicode_wrapping_and_cross_page_copy() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process("old 界e\u{301}\r\nabcdefghijklmnop\r\nlast\r\nfinal".as_bytes());
        let mut h = History::new(parser.screen(), true);
        h.text_input("abcdefghijklmnop");
        h.key("enter", 0);
        assert_eq!(h.matches.len(), 1);
        assert_eq!(h.selected_text().as_deref(), Some("abcdefghijklmnop"));
        h.key("char:/", 0);
        h.key("char:u", 2);
        h.text_input("界e\u{301}");
        h.key("enter", 0);
        assert_eq!(h.matches.len(), 1);
        assert_eq!(h.selected_text().as_deref(), Some("界e\u{301}"));
        h.key("home", 2);
        h.key("space", 0);
        h.key("pagedown", 0);
        h.key("end", 0);
        let text = h.selected_text().unwrap();
        assert!(
            text.starts_with("old 界e\u{301}\nabcdefghijklmnop"),
            "{text:?}"
        );
        parser.process(b"\r\nNEW OUTPUT");
        assert!(!h.text.contains("NEW OUTPUT"));
        assert!(parser.screen().contents().contains("NEW OUTPUT"));
    }
    #[test]
    fn repeated_search_wraps_and_failed_search_does_not_copy_stale_match() {
        let mut parser = vt100::Parser::new(3, 20, 10);
        parser.process(b"one HIT\r\ntwo HIT\r\nthree");
        let mut h = History::new(parser.screen(), true);
        h.text_input("HIT");
        h.key("enter", 0);
        assert_eq!(h.matches.len(), 2);
        h.key("char:n", 0);
        assert!(h.wrapped);
        h.key("char:N", 1);
        assert!(h.wrapped);
        h.key("char:/", 0);
        h.key("char:u", 2);
        h.text_input("absent");
        h.key("enter", 0);
        assert!(h.matches.is_empty());
        assert!(h.selected_text().is_none());
        assert!(matches!(h.key("escape", 0), Outcome::Exit));
    }
    #[test]
    fn query_editor_preserves_shift_spaces_cancel_and_clipboard_shortcut() {
        let mut parser = vt100::Parser::new(2, 20, 10);
        parser.process(b"A B!");
        let mut h = History::new(parser.screen(), true);
        h.key("char:a", 1);
        h.key("space", 0);
        h.key("char:b", 1);
        h.key("char:1", 1);
        h.key("enter", 0);
        assert_eq!(h.query, "A B!");
        assert_eq!(h.matches.len(), 1);
        h.key("char:/", 0);
        h.text_input("uncommitted");
        h.key("escape", 0);
        assert_eq!(h.status().query, "A B!");
        assert!(matches!(h.key("char:c",3),Outcome::Copy(t) if t=="A B!"));
        h.key("char:/", 0);
        assert!(matches!(h.key("char:c", 2), Outcome::Exit));
    }
    #[test]
    fn selection_from_trailing_space_preserves_empty_lines() {
        let mut parser = vt100::Parser::new(3, 10, 10);
        parser.process(b"a\r\n\r\nb");
        let mut h = History::new(parser.screen(), false);
        h.key("home", 2);
        h.key("end", 0);
        h.key("space", 0);
        h.key("down", 0);
        h.key("down", 0);
        h.key("home", 0);
        assert_eq!(h.selected_text().as_deref(), Some("\n\nb"));
    }
}
