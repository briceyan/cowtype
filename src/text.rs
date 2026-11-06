//! Message layout: wrapping, tab handling, and width measurement.
//!
//! `cowsay` builds its balloon from Perl's `Text::Wrap` and `Text::Tabs`. This
//! module ports the parts of those modules that affect cowsay output, so a
//! wrapped message comes out byte for byte identical to the Perl original.
//!
//! One deliberate difference: widths are measured with `unicode-width`, which
//! counts a CJK character as two columns. Perl counts code points, so `cowsay`
//! draws a crooked balloon around non-ASCII text. Everything ASCII-only, which
//! is what cow art and most messages are, behaves exactly like `cowsay`.

use unicode_width::UnicodeWidthChar;

/// Tab stops, fixed at 8 by `cowsay` (`$Text::Wrap::tabstop = 8`).
const TABSTOP: usize = 8;

/// Width of `s` in terminal columns.
pub fn width(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Whitespace as Perl's `\s` sees it without a `/u` modifier.
pub fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// A base character plus any combining marks that follow it.
///
/// Perl's wrap regex measures text in `\PM\pM*` units for exactly this reason:
/// combining marks ride along with the preceding character. Keeping the same
/// unit lets us keep its line breaking rules.
struct Unit {
    text: String,
    space: bool,
    newline: bool,
}

fn units(s: &str) -> Vec<Unit> {
    let mut out: Vec<Unit> = Vec::new();
    for c in s.chars() {
        let cols = c.width().unwrap_or(0);
        let combining = cols == 0 && !c.is_control() && !out.is_empty();
        if combining {
            if let Some(last) = out.last_mut() {
                last.text.push(c);
                continue;
            }
        }
        out.push(Unit {
            text: c.to_string(),
            space: is_space(c),
            newline: c == '\n',
        });
    }
    out
}

fn join(units: &[Unit]) -> String {
    let mut s = String::new();
    for u in units {
        s.push_str(&u.text);
    }
    s
}

/// Port of `Text::Tabs::expand`: tabs advance to the next tab stop.
pub fn expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + TABSTOP);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut col = 0usize;
        for (j, segment) in line.split('\t').enumerate() {
            if j > 0 {
                let pad = TABSTOP - col % TABSTOP;
                out.extend(std::iter::repeat(' ').take(pad));
                col += pad;
            }
            out.push_str(segment);
            col += width(segment);
        }
    }
    out
}

/// Port of `Text::Tabs::unexpand`.
///
/// `cowsay` leaves `$Text::Wrap::unexpand` at its default (on), so a run of two
/// or more trailing spaces inside a tab-stop-wide chunk comes back out as a
/// literal tab. Messages can therefore contain tabs, and matching that keeps
/// our output byte-identical.
pub fn unexpand(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let expanded = expand(line);

        // Re-cut the line into tab-stop-wide chunks.
        let mut chunks: Vec<String> = Vec::new();
        let mut chunk = String::new();
        let mut cols = 0usize;
        for c in expanded.chars() {
            let w = c.width().unwrap_or(0);
            if cols > 0 && cols + w > TABSTOP {
                chunks.push(std::mem::take(&mut chunk));
                cols = 0;
            }
            chunk.push(c);
            cols += w;
        }
        chunks.push(chunk);

        for chunk in &chunks {
            let trimmed = chunk.trim_end_matches(' ');
            if chunk.chars().count() - trimmed.chars().count() >= 2 {
                out.push_str(trimmed);
                out.push('\t');
            } else {
                out.push_str(chunk);
            }
        }
    }
    out
}

/// Perl's `split /\n\s+/`, used by `Text::Wrap::fill` to find paragraphs.
fn split_paragraphs(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\n' {
            let mut j = i + 1;
            while j < chars.len() && is_space(chars[j]) {
                j += 1;
            }
            if j > i + 1 {
                parts.push(std::mem::take(&mut current));
                i = j;
                continue;
            }
        }
        current.push(chars[i]);
        i += 1;
    }
    parts.push(current);
    while parts.last().map(|p| p.is_empty()).unwrap_or(false) {
        parts.pop();
    }
    parts
}

/// Port of `s/\s+/ /g`, applied to every paragraph by `fill`.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_space = false;
    for c in s.chars() {
        if is_space(c) {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// Port of `Text::Wrap::fill('', '', @lines)` followed by `split "\n"`.
pub fn fill(lines: &[String], columns: usize) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    let joined = lines.join("\n");
    let mut paragraphs: Vec<String> = Vec::new();
    for paragraph in split_paragraphs(&joined) {
        paragraphs.push(wrap_paragraph(&collapse(&paragraph), columns));
    }
    split_lines(&paragraphs.join("\n\n"))
}

/// Perl's `split("\n", $s)`: trailing empty fields are dropped and an empty
/// string splits into nothing at all.
pub fn split_lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = s.split('\n').map(str::to_string).collect();
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

/// Port of `Text::Wrap::wrap('', '', $paragraph)` for one paragraph.
///
/// `cowsay` only ever wraps with empty indents, which collapses the Perl
/// routine to: break greedily at the last whitespace that fits, and hard-break
/// words that are longer than a line.
fn wrap_paragraph(text: &str, columns: usize) -> String {
    // Perl: $columns - length($ip) - 1, i.e. one column is kept for the newline.
    let limit = columns.saturating_sub(1);
    let units = units(text);
    let mut out = String::new();
    let mut pos = 0usize;
    let mut first_line = true;
    let mut tail = String::new();
    let mut current = String::new();

    while pos < units.len() {
        // Perl's loop guard consumes a whitespace-only remainder and stops, so
        // a message of nothing but spaces renders as an empty balloon.
        if units[pos..].iter().all(|u| u.space) {
            pos = units.len();
            break;
        }
        let mut placed = false;

        // Greedy break: scan back from the widest line that fits for a place
        // where the next unit is whitespace, a newline run, or the end.
        let mut j = (pos + limit).min(units.len());
        loop {
            let boundary = j == units.len() || units[j].newline || units[j].space;
            if boundary {
                current = join(&units[pos..j]);
                if j == units.len() {
                    tail.clear();
                    pos = j;
                } else if units[j].newline {
                    let mut k = j;
                    while k < units.len() && units[k].newline {
                        k += 1;
                    }
                    tail = join(&units[j..k]);
                    pos = k;
                } else {
                    // $break consumes exactly one whitespace unit, or a CRLF pair.
                    let mut k = j + 1;
                    if units[j].text == "\r" && k < units.len() && units[k].newline {
                        k += 1;
                    }
                    tail = join(&units[j..k]);
                    pos = k;
                }
                placed = true;
                break;
            }
            if j == pos {
                break;
            }
            j -= 1;
        }

        if !placed {
            // $huge is 'wrap': a word too long for a line is cut at the limit.
            let mut k = pos;
            let mut taken = 0usize;
            while k < units.len() && taken < limit && !units[k].newline {
                k += 1;
                taken += 1;
            }
            if k == pos {
                break;
            }
            current = join(&units[pos..k]);
            tail = "\n".to_string();
            pos = k;
        }

        if !first_line {
            out.push('\n');
        }
        first_line = false;
        out.push_str(&unexpand(&current));
    }

    out.push_str(&tail);
    if pos < units.len() {
        out.push_str(&join(&units[pos..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill1(message: &str, columns: usize) -> Vec<String> {
        fill(&[message.to_string()], columns)
    }

    #[test]
    fn expands_tabs_to_stops() {
        assert_eq!(expand("a\tb"), "a       b");
        assert_eq!(expand("abc\td"), "abc     d");
        assert_eq!(expand("12345678\tx"), "12345678        x");
    }

    #[test]
    fn unexpand_restores_tabs_only_for_two_spaces_or_more() {
        assert_eq!(unexpand("abcdefg "), "abcdefg ");
        assert_eq!(unexpand("abcdef  "), "abcdef\t");
        assert_eq!(unexpand("        "), "\t");
    }

    #[test]
    fn wraps_at_last_space_that_fits() {
        assert_eq!(
            fill1(
                "the quick brown fox jumps over the lazy dog and keeps running far away",
                40
            ),
            vec![
                "the quick brown fox jumps over the lazy".to_string(),
                "dog and keeps running far away".to_string(),
            ]
        );
    }

    #[test]
    fn hard_breaks_long_words() {
        let out = fill1(&"a".repeat(50), 40);
        assert_eq!(out, vec!["a".repeat(39), "a".repeat(11)]);
    }

    #[test]
    fn blank_lines_separate_paragraphs() {
        let lines = vec!["one".to_string(), String::new(), "two".to_string()];
        assert_eq!(fill(&lines, 40), vec!["one", "", "two"]);
    }

    #[test]
    fn whitespace_only_message_becomes_empty() {
        assert!(fill1(" ", 40).is_empty());
        assert!(fill1("\t", 40).is_empty());
        assert_eq!(fill1("abc ", 40), vec!["abc "]);
        assert_eq!(fill1("abc  ", 40), vec!["abc "]);
    }

    #[test]
    fn empty_input_yields_no_lines() {
        assert!(fill(&[], 40).is_empty());
        assert!(fill(&[String::new()], 40).is_empty());
    }

    #[test]
    fn counts_cjk_as_two_columns() {
        assert_eq!(width("中文"), 4);
        assert_eq!(width("abc"), 3);
    }
}
