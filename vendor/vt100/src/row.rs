use crate::term::BufWrite as _;

#[derive(Clone, Debug)]
pub struct Row {
    cells: Vec<crate::Cell>,
    wrapped: bool,
    hyperlinks: Option<Vec<Option<std::sync::Arc<crate::hyperlink::Hyperlink>>>>,
}

impl Row {
    pub fn new(cols: u16) -> Self {
        Self {
            cells: vec![crate::Cell::new(); usize::from(cols)],
            wrapped: false,
            hyperlinks: None,
        }
    }

    fn cols(&self) -> u16 {
        self.cells
            .len()
            .try_into()
            // we limit the number of cols to a u16 (see Size)
            .unwrap()
    }

    pub fn clear(&mut self, attrs: crate::attrs::Attrs) {
        self.hyperlinks = None;
        for cell in &mut self.cells {
            cell.clear(attrs);
        }
        self.wrapped = false;
    }

    pub(crate) fn cells(&self) -> impl Iterator<Item = &crate::Cell> {
        self.cells.iter()
    }

    pub fn get(&self, col: u16) -> Option<&crate::Cell> {
        self.cells.get(usize::from(col))
    }

    pub fn get_mut(&mut self, col: u16) -> Option<&mut crate::Cell> {
        self.cells.get_mut(usize::from(col))
    }

    pub(crate) fn hyperlink(&self, col: u16) -> Option<&crate::hyperlink::Hyperlink> {
        self.hyperlinks.as_ref()?.get(usize::from(col))?.as_deref()
    }

    #[inline]
    fn set_hyperlink(
        &mut self,
        col: u16,
        link: Option<std::sync::Arc<crate::hyperlink::Hyperlink>>,
    ) {
        if link.is_some() && self.hyperlinks.is_none() {
            self.hyperlinks = Some(vec![None; self.cells.len()]);
        }
        if let Some(links) = &mut self.hyperlinks {
            links[usize::from(col)] = link;
        }
    }

    /// Paint text and link metadata in one row lookup. Unlinked rows keep the
    /// upstream POD cells and require no per-cell reference-count operations.
    #[inline]
    pub(crate) fn paint(
        &mut self,
        col: u16,
        c: char,
        width: u16,
        attrs: crate::attrs::Attrs,
        link: Option<std::sync::Arc<crate::hyperlink::Hyperlink>>,
    ) {
        let i = usize::from(col);
        let previous_continuation = self.cells[i].is_wide_continuation();
        let previous_wide = self.cells[i].is_wide();
        if previous_continuation {
            self.cells[i - 1].clear(attrs);
        }
        if previous_wide {
            self.cells[i + 1].set(' ', 1, attrs);
        }
        self.cells[i].set(c, width, attrs);
        let next_wide = width > 1 && self.cells[i + 1].is_wide();
        if width > 1 {
            if next_wide {
                self.cells[i + 2].clear(attrs);
                if i + 2 == self.cells.len() - 1 {
                    self.wrapped = false;
                }
            }
            self.cells[i + 1].clear(crate::attrs::Attrs::default());
            self.cells[i + 1].set_wide_continuation(true);
        }
        // Most output has no links. Do not clone/drop optional Arc values or
        // visit a metadata vector for any cell on that path.
        if self.hyperlinks.is_none() && link.is_none() {
            return;
        }
        if previous_continuation {
            self.set_hyperlink(col - 1, None);
        }
        if previous_wide {
            self.set_hyperlink(col + 1, None);
        }
        if width > 1 {
            if next_wide {
                self.set_hyperlink(col + 2, None);
            }
            self.set_hyperlink(col + 1, link.clone());
        }
        self.set_hyperlink(col, link);
    }

    pub fn insert(&mut self, i: u16, cell: crate::Cell) {
        self.cells.insert(usize::from(i), cell);
        if let Some(links) = &mut self.hyperlinks {
            links.insert(usize::from(i), None);
        }
        self.wrapped = false;
    }

    pub fn remove(&mut self, i: u16) {
        self.clear_wide(i);
        self.cells.remove(usize::from(i));
        if let Some(links) = &mut self.hyperlinks {
            links.remove(usize::from(i));
        }
        self.wrapped = false;
    }

    pub fn erase(&mut self, i: u16, attrs: crate::attrs::Attrs) {
        let wide = self.cells[usize::from(i)].is_wide();
        self.clear_wide(i);
        self.cells[usize::from(i)].clear(attrs);
        self.set_hyperlink(i, None);
        if i == self.cols() - if wide { 2 } else { 1 } {
            self.wrapped = false;
        }
    }

    pub fn truncate(&mut self, len: u16) {
        self.cells.truncate(usize::from(len));
        if let Some(links) = &mut self.hyperlinks {
            links.truncate(usize::from(len));
        }
        self.wrapped = false;
        let last_cell = &mut self.cells[usize::from(len) - 1];
        if last_cell.is_wide() {
            last_cell.clear(*last_cell.attrs());
            self.set_hyperlink(len - 1, None);
        }
    }

    pub fn resize(&mut self, len: u16, cell: crate::Cell) {
        self.cells.resize(usize::from(len), cell);
        if let Some(links) = &mut self.hyperlinks {
            links.resize(usize::from(len), None);
        }
        self.wrapped = false;
    }

    pub fn wrap(&mut self, wrap: bool) {
        self.wrapped = wrap;
    }

    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    pub fn clear_wide(&mut self, col: u16) {
        let cell = &self.cells[usize::from(col)];
        let other_col = if cell.is_wide() {
            col + 1
        } else if cell.is_wide_continuation() {
            col - 1
        } else {
            return;
        };
        let other = &mut self.cells[usize::from(other_col)];
        other.clear(*other.attrs());
        self.set_hyperlink(other_col, None);
    }

    pub fn write_contents(&self, contents: &mut String, start: u16, width: u16, wrapping: bool) {
        let mut prev_was_wide = false;

        let mut prev_col = start;
        for (col, cell) in self
            .cells()
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            if cell.has_contents() {
                for _ in 0..(col - prev_col) {
                    contents.push(' ');
                }
                prev_col += col - prev_col;

                contents.push_str(cell.contents());
                prev_col += if cell.is_wide() { 2 } else { 1 };
            }
        }
        if prev_col == start && wrapping {
            contents.push('\n');
        }
    }

    pub fn write_contents_formatted(
        &self,
        contents: &mut Vec<u8>,
        start: u16,
        width: u16,
        row: u16,
        wrapping: bool,
        prev_pos: Option<crate::grid::Pos>,
        prev_attrs: Option<crate::attrs::Attrs>,
        limit: usize,
    ) -> Option<(crate::grid::Pos, crate::attrs::Attrs)> {
        let mut prev_was_wide = false;
        let mut prev_link = None;
        let default_cell = crate::Cell::new();

        let mut prev_pos = prev_pos.unwrap_or_else(|| {
            if wrapping {
                crate::grid::Pos {
                    row: row - 1,
                    col: self.cols(),
                }
            } else {
                crate::grid::Pos { row, col: start }
            }
        });
        let mut prev_attrs = prev_attrs.unwrap_or_default();

        let first_cell = &self.cells[usize::from(start)];
        if wrapping && first_cell == &default_cell {
            let default_attrs = default_cell.attrs();
            if &prev_attrs != default_attrs {
                default_attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *default_attrs;
            }
            contents.push(b' ');
            crate::term::Backspace.write_buf(contents);
            crate::term::EraseChar::new(1).write_buf(contents);
            prev_pos = crate::grid::Pos { row, col: 0 };
        }

        let mut erase: Option<(u16, &crate::attrs::Attrs)> = None;
        for (col, cell) in self
            .cells()
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            let next_link = self.hyperlink(col as u16);
            let header = if prev_link != next_link {
                next_link.map_or(0, |h: &crate::hyperlink::Hyperlink| {
                    h.uri.len() + h.params.len() + 16
                })
            } else {
                0
            };
            if contents
                .len()
                .saturating_add(header)
                .saturating_add(usize::from(self.cols()) + 256)
                > limit
            {
                return None;
            }
            if prev_link != next_link {
                crate::hyperlink::write_diff(contents, &mut prev_link, None);
            }
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            let pos = crate::grid::Pos { row, col };

            if let Some((prev_col, attrs)) = erase {
                crate::hyperlink::write_diff(contents, &mut prev_link, None);
                if cell.has_contents() || cell.attrs() != attrs {
                    let new_pos = crate::grid::Pos { row, col: prev_col };
                    if wrapping && prev_pos.row + 1 == new_pos.row && prev_pos.col >= self.cols() {
                        if new_pos.col > 0 {
                            contents.extend(" ".repeat(usize::from(new_pos.col)).as_bytes());
                        } else {
                            contents.extend(b" ");
                            crate::term::Backspace.write_buf(contents);
                        }
                    } else {
                        crate::term::MoveFromTo::new(prev_pos, new_pos).write_buf(contents);
                    }
                    prev_pos = new_pos;
                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }
                    crate::term::EraseChar::new(pos.col - prev_col).write_buf(contents);
                    erase = None;
                }
            }

            if cell != &default_cell {
                let attrs = cell.attrs();
                if cell.has_contents() {
                    if pos != prev_pos {
                        crate::hyperlink::write_diff(contents, &mut prev_link, None);
                        if !wrapping
                            || prev_pos.row + 1 != pos.row
                            || prev_pos.col < self.cols() - u16::from(cell.is_wide())
                            || pos.col != 0
                        {
                            crate::term::MoveFromTo::new(prev_pos, pos).write_buf(contents);
                        }
                        prev_pos = pos;
                    }

                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }

                    prev_pos.col += if cell.is_wide() { 2 } else { 1 };
                    let cell_contents = cell.contents();
                    crate::hyperlink::write_diff(
                        contents,
                        &mut prev_link,
                        self.hyperlink(col as u16),
                    );
                    contents.extend(cell_contents.as_bytes());
                } else if erase.is_none() {
                    erase = Some((pos.col, attrs));
                }
            }
        }
        if let Some((prev_col, attrs)) = erase {
            crate::hyperlink::write_diff(contents, &mut prev_link, None);
            let new_pos = crate::grid::Pos { row, col: prev_col };
            if wrapping && prev_pos.row + 1 == new_pos.row && prev_pos.col >= self.cols() {
                if new_pos.col > 0 {
                    contents.extend(" ".repeat(usize::from(new_pos.col)).as_bytes());
                } else {
                    contents.extend(b" ");
                    crate::term::Backspace.write_buf(contents);
                }
            } else {
                crate::term::MoveFromTo::new(prev_pos, new_pos).write_buf(contents);
            }
            prev_pos = new_pos;
            if &prev_attrs != attrs {
                attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *attrs;
            }
            crate::term::ClearRowForward.write_buf(contents);
        }

        crate::hyperlink::write_diff(contents, &mut prev_link, None);
        Some((prev_pos, prev_attrs))
    }

    // while it's true that most of the logic in this is identical to
    // write_contents_formatted, i can't figure out how to break out the
    // common parts without making things noticeably slower.
    pub fn write_contents_diff(
        &self,
        contents: &mut Vec<u8>,
        prev: &Self,
        start: u16,
        width: u16,
        row: u16,
        wrapping: bool,
        prev_wrapping: bool,
        mut prev_pos: crate::grid::Pos,
        mut prev_attrs: crate::attrs::Attrs,
    ) -> (crate::grid::Pos, crate::attrs::Attrs) {
        let mut prev_was_wide = false;
        let mut prev_link = None;

        let first_cell = &self.cells[usize::from(start)];
        let prev_first_cell = &prev.cells[usize::from(start)];
        if wrapping
            && !prev_wrapping
            && first_cell == prev_first_cell
            && self.hyperlink(start) == prev.hyperlink(start)
            && prev_pos.row + 1 == row
            && prev_pos.col >= self.cols() - u16::from(prev_first_cell.is_wide())
        {
            let first_cell_attrs = first_cell.attrs();
            if &prev_attrs != first_cell_attrs {
                first_cell_attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *first_cell_attrs;
            }
            let mut cell_contents = prev_first_cell.contents();
            let need_erase = if cell_contents.is_empty() {
                cell_contents = " ";
                true
            } else {
                false
            };
            crate::hyperlink::write_diff(contents, &mut prev_link, prev.hyperlink(start));
            contents.extend(cell_contents.as_bytes());
            crate::hyperlink::write_diff(contents, &mut prev_link, None);
            crate::term::Backspace.write_buf(contents);
            if prev_first_cell.is_wide() {
                crate::term::Backspace.write_buf(contents);
            }
            if need_erase {
                crate::term::EraseChar::new(1).write_buf(contents);
            }
            prev_pos = crate::grid::Pos { row, col: 0 };
        }

        let mut erase: Option<(u16, &crate::attrs::Attrs)> = None;
        for (col, (cell, prev_cell)) in self
            .cells()
            .zip(prev.cells())
            .enumerate()
            .skip(usize::from(start))
            .take(usize::from(width))
        {
            if prev_link != self.hyperlink(col as u16) {
                crate::hyperlink::write_diff(contents, &mut prev_link, None);
            }
            if prev_was_wide {
                prev_was_wide = false;
                continue;
            }
            prev_was_wide = cell.is_wide();

            // we limit the number of cols to a u16 (see Size)
            let col: u16 = col.try_into().unwrap();
            let pos = crate::grid::Pos { row, col };

            if let Some((prev_col, attrs)) = erase {
                crate::hyperlink::write_diff(contents, &mut prev_link, None);
                if cell.has_contents() || cell.attrs() != attrs {
                    let new_pos = crate::grid::Pos { row, col: prev_col };
                    if wrapping && prev_pos.row + 1 == new_pos.row && prev_pos.col >= self.cols() {
                        if new_pos.col > 0 {
                            contents.extend(" ".repeat(usize::from(new_pos.col)).as_bytes());
                        } else {
                            contents.extend(b" ");
                            crate::term::Backspace.write_buf(contents);
                        }
                    } else {
                        crate::term::MoveFromTo::new(prev_pos, new_pos).write_buf(contents);
                    }
                    prev_pos = new_pos;
                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }
                    crate::term::EraseChar::new(pos.col - prev_col).write_buf(contents);
                    erase = None;
                }
            }

            if cell != prev_cell || self.hyperlink(col) != prev.hyperlink(col) {
                let attrs = cell.attrs();
                if cell.has_contents() {
                    if pos != prev_pos {
                        crate::hyperlink::write_diff(contents, &mut prev_link, None);
                        if !wrapping
                            || prev_pos.row + 1 != pos.row
                            || prev_pos.col < self.cols() - u16::from(cell.is_wide())
                            || pos.col != 0
                        {
                            crate::term::MoveFromTo::new(prev_pos, pos).write_buf(contents);
                        }
                        prev_pos = pos;
                    }

                    if &prev_attrs != attrs {
                        attrs.write_escape_code_diff(contents, &prev_attrs);
                        prev_attrs = *attrs;
                    }

                    prev_pos.col += if cell.is_wide() { 2 } else { 1 };
                    crate::hyperlink::write_diff(
                        contents,
                        &mut prev_link,
                        self.hyperlink(col as u16),
                    );
                    contents.extend(cell.contents().as_bytes());
                } else if erase.is_none() {
                    erase = Some((pos.col, attrs));
                }
            }
        }
        if let Some((prev_col, attrs)) = erase {
            crate::hyperlink::write_diff(contents, &mut prev_link, None);
            let new_pos = crate::grid::Pos { row, col: prev_col };
            if wrapping && prev_pos.row + 1 == new_pos.row && prev_pos.col >= self.cols() {
                if new_pos.col > 0 {
                    contents.extend(" ".repeat(usize::from(new_pos.col)).as_bytes());
                } else {
                    contents.extend(b" ");
                    crate::term::Backspace.write_buf(contents);
                }
            } else {
                crate::term::MoveFromTo::new(prev_pos, new_pos).write_buf(contents);
            }
            prev_pos = new_pos;
            if &prev_attrs != attrs {
                attrs.write_escape_code_diff(contents, &prev_attrs);
                prev_attrs = *attrs;
            }
            crate::term::ClearRowForward.write_buf(contents);
        }

        // if this row is going from wrapped to not wrapped, we need to erase
        // and redraw the last character to break wrapping. if this row is
        // wrapped, we need to redraw the last character without erasing it to
        // position the cursor after the end of the line correctly so that
        // drawing the next line can just start writing and be wrapped.
        if (!self.wrapped && prev.wrapped) || (!prev.wrapped && self.wrapped) {
            let end_pos = if self.cells[usize::from(self.cols() - 1)].is_wide_continuation() {
                crate::grid::Pos {
                    row,
                    col: self.cols() - 2,
                }
            } else {
                crate::grid::Pos {
                    row,
                    col: self.cols() - 1,
                }
            };
            crate::term::MoveFromTo::new(prev_pos, end_pos).write_buf(contents);
            prev_pos = end_pos;
            if !self.wrapped {
                crate::term::EraseChar::new(1).write_buf(contents);
            }
            let end_cell = &self.cells[usize::from(end_pos.col)];
            if end_cell.has_contents() {
                let attrs = end_cell.attrs();
                if &prev_attrs != attrs {
                    attrs.write_escape_code_diff(contents, &prev_attrs);
                    prev_attrs = *attrs;
                }
                crate::hyperlink::write_diff(contents, &mut prev_link, self.hyperlink(end_pos.col));
                contents.extend(end_cell.contents().as_bytes());
                prev_pos.col += if end_cell.is_wide() { 2 } else { 1 };
            }
        }

        crate::hyperlink::write_diff(contents, &mut prev_link, None);
        (prev_pos, prev_attrs)
    }
}
