//! Small formatting helpers shared by both domains' reports.

/// Whether to emit ANSI styling.
///
/// Honours `NO_COLOR`, which the demo respects so its output can be piped into
/// a file or a pager without escape codes.
pub fn styled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

/// Bold, when styling is on.
pub fn bold(text: &str) -> String {
    if styled() {
        format!("\u{1b}[1m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}

/// Dimmed, when styling is on.
pub fn dim(text: &str) -> String {
    if styled() {
        format!("\u{1b}[2m{text}\u{1b}[0m")
    } else {
        text.to_owned()
    }
}

/// A section heading for the terminal, ending in a newline so callers can
/// append the section's body directly.
pub fn heading(text: &str) -> String {
    format!("\n{}\n{}\n", bold(text), dim(&"-".repeat(text.chars().count())))
}

/// A number with thousands separators, e.g. `-12,345`.
pub fn thousands(value: f64) -> String {
    let negative = value < 0.0;
    let whole = value.abs().round() as u64;
    let digits = whole.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if negative {
        format!("-{out}")
    } else {
        out
    }
}

/// A signed number with thousands separators, e.g. `+12,345`.
pub fn signed(value: f64) -> String {
    if value >= 0.0 {
        format!("+{}", thousands(value))
    } else {
        thousands(value)
    }
}

/// A horizontal bar, padded to `width` so the column reads left-aligned even
/// though the table right-aligns its value columns.
pub fn bar(count: usize, max: usize, width: usize) -> String {
    if max == 0 {
        return " ".repeat(width);
    }
    let filled = (count * width).div_ceil(max.max(1)).min(width);
    format!("{}{}", "\u{2588}".repeat(filled), " ".repeat(width - filled))
}

/// Lay out rows as an aligned plain-text table, with the first row as headers.
pub fn table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell));
        }
    }
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        let mut line = String::from("  ");
        for (i, cell) in row.iter().enumerate() {
            let pad = widths[i].saturating_sub(display_width(cell));
            if i == 0 {
                line.push_str(cell);
                line.push_str(&" ".repeat(pad));
            } else {
                line.push_str("  ");
                line.push_str(&" ".repeat(pad));
                line.push_str(cell);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
        if index == 0 {
            out.push_str("  ");
            out.push_str(&dim(
                &widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("  "),
            ));
            out.push('\n');
        }
    }
    out
}

/// The same rows as a Markdown table.
pub fn markdown_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        let mut cells: Vec<String> = row.iter().map(|c| escape_pipes(c)).collect();
        while cells.len() < columns {
            cells.push(String::new());
        }
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
        if index == 0 {
            let mut rule = vec!["---".to_owned()];
            rule.extend(std::iter::repeat_n("--:".to_owned(), columns.saturating_sub(1)));
            out.push_str(&format!("| {} |\n", rule.join(" | ")));
        }
    }
    out
}

fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|")
}

/// Character count, ignoring ANSI escapes.
fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in text.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\u{1b}' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

/// Wrap `text` to `width` columns, indenting continuation lines.
pub fn wrap(text: &str, width: usize, indent: &str) -> String {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join(&format!("\n{indent}"))
}
