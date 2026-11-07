//! Shared viewport rules for compact dialogs and navigation.
pub fn field_range(total: usize, selected: usize, capacity: usize) -> std::ops::Range<usize> {
    let capacity = capacity.max(1);
    let start = selected
        .min(total.saturating_sub(1))
        .saturating_sub(capacity - 1)
        .min(total.saturating_sub(capacity));
    start..(start + capacity).min(total)
}
pub fn edit_window(text: &str, cursor: usize, columns: usize) -> String {
    let cursor = cursor.min(text.len());
    let cursor = if text.is_char_boundary(cursor) {
        cursor
    } else {
        text.char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i < cursor)
            .last()
            .unwrap_or(0)
    };
    let budget = columns.saturating_sub(1);
    let after =
        starkit::ratatui::text::Line::from(text[cursor..].chars().take(5).collect::<String>())
            .width()
            .min(budget);
    let before_budget = budget.saturating_sub(after);
    let mut used = 0;
    let mut start = cursor;
    for (i, c) in text[..cursor].char_indices().rev() {
        let width = starkit::ratatui::text::Line::from(c.to_string()).width();
        if used + width > before_budget {
            break;
        }
        used += width;
        start = i;
    }
    let mut result = format!("{}│", &text[start..cursor]);
    for c in text[cursor..].chars() {
        let width = starkit::ratatui::text::Line::from(c.to_string()).width();
        if used + width > budget {
            break;
        }
        used += width;
        result.push(c);
    }
    result
}
pub fn session_rows(width: u16, height: u16, cells: bool) -> usize {
    if cells {
        if width < 60 || height < 16 {
            usize::from(height.saturating_sub(7))
        } else {
            usize::from(height.saturating_sub(9) / 2)
        }
    } else if width < 524 || height < 324 {
        usize::from(650.min(height.saturating_sub(16)).saturating_sub(128) / 28)
    } else {
        usize::from(650.min(height.saturating_sub(24)).saturating_sub(230) / 52)
    }
    .max(1)
}
pub fn git_lines(status: Option<&crate::git::Status>, columns: usize) -> Vec<String> {
    let Some(g) = status else {
        return vec!["Reading Git status…".into()];
    };
    let mut lines = vec![
        g.summary(),
        format!("Root: {}", g.root),
        format!("Upstream: {} · stashes: {}", g.upstream, g.stash),
        format!("Last commit: {}", g.last_commit),
    ];
    if !g.error.is_empty() {
        lines.push(g.error.clone());
    }
    if g.clean() {
        lines.push("Working tree clean".into());
    }
    lines.extend(g.files.iter().map(|f| {
        format!(
            "{}{} {}{}",
            f.index,
            f.worktree,
            if f.conflict { "CONFLICT " } else { "" },
            f.original
                .as_ref()
                .map_or_else(|| f.path.clone(), |old| format!("{old} → {}", f.path))
        )
    }));
    let columns = columns.max(2);
    let mut wrapped = Vec::new();
    for line in lines {
        let mut text = String::new();
        let mut used = 0;
        for c in line.chars().filter(|c| !c.is_control()) {
            let width = starkit::ratatui::text::Line::from(c.to_string()).width();
            if used + width > columns && !text.is_empty() {
                wrapped.push(std::mem::take(&mut text));
                used = 0;
            }
            text.push(c);
            used += width;
        }
        wrapped.push(text);
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_field_is_visible_and_utf8_cursors_stay_inside_the_window() {
        for total in 1..12 {
            for capacity in 1..6 {
                for selected in 0..total {
                    let r = field_range(total, selected, capacity);
                    assert!(r.contains(&selected));
                    assert!(r.end <= total && r.len() <= capacity);
                }
            }
        }
        let text = "long path with café/世界/last";
        for (cursor, _) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            let window = edit_window(text, cursor, 10);
            assert!(window.contains('│'));
            assert!(starkit::ratatui::text::Line::from(window).width() <= 10);
        }
    }
}
