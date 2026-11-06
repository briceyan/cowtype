//! Cow files: locating them, listing them, and reading them.
//!
//! cowsay cow files are Perl snippets, and cowsay loads them with `do`. Running
//! Perl is not an option here, so this module understands the small language
//! that cow files actually use:
//!
//! ```text
//! $the_cow = <<EOC;            # heredoc template, interpolating
//! $the_cow = <<'EOC';          # heredoc template, literal backslashes
//! $the_cow = <<"A" . <<'B';    # concatenated heredocs
//! $eyes = 'xx';                # override the face
//! $eyes .= ' o';               # append to the face
//! $other_eye = chop($eyes);    # chop/chomp on a face variable
//! $balloon_lines[0] =~ tr/_/-/;# transliterate a balloon line
//! unshift @balloon_lines, "\n";# add a balloon line
//! push @balloon_lines, "x";
//! ```
//!
//! Anything else is reported as a warning and skipped, so a cow that relies on
//! real Perl degrades to its template instead of failing.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// A post-processing step a cow file applies to the balloon.
#[derive(Debug, Clone)]
pub enum BalloonOp {
    /// `$balloon_lines[INDEX] =~ tr/FROM/TO/`
    Translate {
        index: isize,
        from: Vec<char>,
        to: Vec<char>,
    },
    /// `unshift`/`push @balloon_lines, "..."`
    Insert { front: bool, lines: Vec<String> },
}

/// A cow file, ready to print.
#[derive(Debug, Default)]
pub struct ParsedCow {
    /// Cow art with faces already substituted.
    pub template: String,
    pub ops: Vec<BalloonOp>,
    pub warnings: Vec<String>,
}

impl ParsedCow {
    /// Apply the cow's balloon patches, in file order.
    pub fn patch_balloon(&self, lines: &mut Vec<String>) {
        for op in &self.ops {
            match op {
                BalloonOp::Translate { index, from, to } => {
                    let len = lines.len() as isize;
                    let idx = if *index < 0 { len + *index } else { *index };
                    if idx < 0 || idx >= len {
                        continue;
                    }
                    let mut rebuilt = String::new();
                    for c in lines[idx as usize].chars() {
                        match from.iter().position(|f| *f == c) {
                            Some(position) => {
                                if let Some(replacement) = to.get(position).or_else(|| to.last()) {
                                    rebuilt.push(*replacement);
                                }
                            }
                            None => rebuilt.push(c),
                        }
                    }
                    lines[idx as usize] = rebuilt;
                }
                BalloonOp::Insert {
                    front,
                    lines: added,
                } => {
                    if *front {
                        for line in added.iter().rev() {
                            lines.insert(0, line.clone());
                        }
                    } else {
                        for line in added {
                            lines.push(line.clone());
                        }
                    }
                }
            }
        }
    }
}

/// Directories searched for cow files, in the same order as cowsay.
///
/// cowsay derives `<prefix>/share/cowsay` from the location of its own script
/// and adds `etc/cowsay/cowpath.d/*.path` and `$COWPATH`. The extra
/// `/usr/local` and `/usr/share` guesses let a cargo-installed binary find the
/// cows that a package manager installed.
pub fn cowpath() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin) = exe.parent() {
            if let Some(prefix) = bin.parent() {
                dirs.push(prefix.join("share/cowsay/site-cows"));
                dirs.push(prefix.join("share/cowsay/cows"));
                read_cowpath_dir(&prefix.join("etc/cowsay/cowpath.d"), &mut dirs);
            }
        }
    }

    read_cowpath_dir(Path::new("/etc/cowsay/cowpath.d"), &mut dirs);
    dirs.push(PathBuf::from("/usr/local/share/cowsay/cows"));
    dirs.push(PathBuf::from("/usr/local/share/cows"));
    dirs.push(PathBuf::from("/usr/share/cowsay/cows"));
    dirs.push(PathBuf::from("/opt/homebrew/share/cowsay/cows"));

    if let Some(user) = std::env::var_os("COWPATH") {
        let only_user = std::env::var("COWSAY_ONLY_COWPATH").as_deref() == Ok("1");
        let entries: Vec<PathBuf> = std::env::split_paths(&user).collect();
        if only_user {
            dirs = entries;
        } else {
            dirs.extend(entries);
        }
    }

    let mut seen = BTreeSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

fn read_cowpath_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("path") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    out.push(PathBuf::from(line));
                }
            }
        }
    }
}

/// Find a cow by name or by path, the way `-f` does.
pub fn resolve(name: &str, path: &[PathBuf]) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.is_file() {
        return Some(direct.to_path_buf());
    }
    for dir in path {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        let with_edge = dir.join(format!("{name}.cow"));
        if with_edge.is_file() {
            return Some(with_edge);
        }
    }
    None
}

/// Every cow name visible on the cowpath, sorted and de-duplicated.
pub fn list(path: &[PathBuf]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for dir in path {
        collect(dir, dir, &mut names);
    }
    names.into_iter().collect()
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("cow") {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(root) {
            let mut name = relative.to_string_lossy().into_owned();
            name.truncate(name.len() - ".cow".len());
            out.insert(name);
        }
    }
}

/// Render `-l` output, matching cowsay: a wrapped column when stdout is a
/// terminal, one name per line when it is piped.
pub fn render_list(path: &[PathBuf], columns: usize, pretty: bool) -> String {
    let names = list(path);
    if !pretty {
        let mut out = String::new();
        for name in names {
            out.push_str(&name);
            out.push('\n');
        }
        return out;
    }

    let mut out = String::new();
    let mut first = true;
    for dir in path {
        let mut here: Vec<String> = Vec::new();
        let mut all = BTreeSet::new();
        collect(dir, dir, &mut all);
        here.extend(all);
        if here.is_empty() {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&format!("Cow files in {}:\n", dir.display()));
        let joined = here.join(" ");
        let wrapped = crate::text::fill(&[joined], columns).join("\n");
        out.push_str(&wrapped);
        out.push('\n');
    }
    out
}

/// Pick a random cow name, honouring `-C` (include truecolor cows).
pub fn random_name(path: &[PathBuf], full_color: bool) -> Option<String> {
    let names: Vec<String> = list(path)
        .into_iter()
        .filter(|n| full_color || !n.starts_with("truecolor/"))
        .collect();
    if names.is_empty() {
        return None;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_f491_4f6c_dd1d);
    // xorshift64*, so a random cow needs no dependency.
    let mut x = seed | 1;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let index = (x.wrapping_mul(0x2545_f491_4f6c_dd1d) % names.len() as u64) as usize;
    Some(names[index].clone())
}

/// Variables visible to a cow template.
struct Vars {
    values: HashMap<String, String>,
}

impl Vars {
    fn new(eyes: &str, tongue: &str, thoughts: &str) -> Self {
        let mut values = HashMap::new();
        values.insert("eyes".to_string(), eyes.to_string());
        values.insert("tongue".to_string(), tongue.to_string());
        values.insert("thoughts".to_string(), thoughts.to_string());
        Vars { values }
    }

    fn get(&self, name: &str) -> Option<&String> {
        self.values.get(name)
    }

    fn set(&mut self, name: &str, value: String) {
        self.values.insert(name.to_string(), value);
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut String> {
        self.values.get_mut(name)
    }

    fn append(&mut self, name: &str, value: &str) {
        self.values
            .entry(name.to_string())
            .or_default()
            .push_str(value);
    }
}

/// Split a Perl heredoc opener at `start`, returning the terminator, whether
/// the body interpolates, and the index just past the opener.
fn heredoc_opener(chars: &[char], start: usize) -> Option<(String, bool, usize)> {
    if start + 1 >= chars.len() || chars[start] != '<' || chars[start + 1] != '<' {
        return None;
    }
    let mut i = start + 2;
    let mut interpolate = true;
    if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
        interpolate = chars[i] == '"';
        i += 1;
    }
    let mut term = String::new();
    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
        term.push(chars[i]);
        i += 1;
    }
    if term.is_empty() {
        return None;
    }
    if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
        i += 1;
    }
    Some((term, interpolate, i))
}

/// Read a heredoc body starting at `start`, returning the body and the index of
/// the line after the terminator.
fn heredoc_body(lines: &[String], start: usize, term: &str) -> (String, usize) {
    let mut body = String::new();
    let mut i = start;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == term || trimmed == format!("{term};") {
            return (body, i + 1);
        }
        body.push_str(&lines[i]);
        body.push('\n');
        i += 1;
    }
    (body, i)
}

/// Expand Perl double-quoted escapes and `$variables`.
fn interpolate(s: &str, vars: &Vars) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            let escaped = match next {
                'n' => Some('\n'),
                't' => Some('\t'),
                'r' => Some('\r'),
                'e' => Some('\x1b'),
                'a' => Some('\x07'),
                'b' => Some('\x08'),
                'f' => Some('\x0c'),
                '\\' => Some('\\'),
                '"' => Some('"'),
                '\'' => Some('\''),
                '$' => Some('$'),
                '@' => Some('@'),
                _ => None,
            };
            match escaped {
                Some(ch) => out.push(ch),
                // Perl keeps unknown escapes verbatim, backslash included.
                None => {
                    out.push('\\');
                    out.push(next);
                }
            }
            i += 2;
            continue;
        }
        if c == '$' {
            let (name, end) = read_var_name(&chars, i + 1);
            if let Some(name) = name {
                if let Some(value) = vars.get(&name) {
                    out.push_str(value);
                    i = end;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Read `$name` or `${name}` after the `$`.
fn read_var_name(chars: &[char], start: usize) -> (Option<String>, usize) {
    if start >= chars.len() {
        return (None, start);
    }
    if chars[start] == '{' {
        let mut i = start + 1;
        let mut name = String::new();
        while i < chars.len() && chars[i] != '}' {
            name.push(chars[i]);
            i += 1;
        }
        if i < chars.len() {
            return (Some(name), i + 1);
        }
        return (None, start);
    }
    let mut i = start;
    let mut name = String::new();
    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
        name.push(chars[i]);
        i += 1;
    }
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return (None, start);
    }
    (Some(name), i)
}

/// Perl single-quoted strings only understand `\\` and `\'`.
fn unescape_single(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\'') => out.push('\''),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Read a quoted string at the start of `s`, returning its value and the rest.
fn take_quoted(s: &str) -> Option<(String, bool, &str)> {
    let mut iter = s.char_indices();
    let (_, quote) = iter.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut escaped = false;
    for (idx, c) in iter {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == quote {
            let value = &s[quote.len_utf8()..idx];
            return Some((value.to_string(), quote == '\'', &s[idx + c.len_utf8()..]));
        }
    }
    None
}

/// Evaluate a value in a cow file: a literal, a variable, or a repetition.
fn eval_atom(expr: &str, vars: &Vars) -> Option<String> {
    let expr = expr.trim();

    // `($extra x 2)` repeats a string; three-eyes.cow builds its eyes this way.
    if let Some(inner) = expr
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        if let Some((value, count)) = inner.rsplit_once(" x ") {
            let value = eval_atom(value, vars)?;
            let count: usize = count.trim().parse().ok()?;
            return Some(value.repeat(count));
        }
        return eval_atom(inner, vars);
    }

    if let Some((value, single, _)) = take_quoted(expr) {
        return Some(if single {
            unescape_single(&value)
        } else {
            interpolate(&value, vars)
        });
    }

    let chars: Vec<char> = expr.chars().collect();
    if chars.first() == Some(&'$') {
        let (name, _) = read_var_name(&chars, 1);
        return name.and_then(|n| vars.get(&n).cloned());
    }

    if expr.is_empty() {
        return Some(String::new());
    }
    Some(expr.to_string())
}

/// Evaluate the right-hand side of an assignment.
fn eval_expr(expr: &str, vars: &mut Vars) -> Option<String> {
    let expr = expr.trim().trim_end_matches(';').trim();

    for (func, keep) in [("chop", false), ("chomp", true)] {
        if let Some(rest) = expr.strip_prefix(func) {
            let inner = rest.trim().trim_start_matches('(').trim_end_matches(')');
            let chars: Vec<char> = inner.chars().collect();
            let (name, _) = read_var_name(&chars, 1);
            let name = name?;
            // chop and chomp modify their argument, and cow files depend on it.
            let current = vars.get_mut(&name)?;
            if keep {
                while current.ends_with('\n') {
                    current.pop();
                }
            } else {
                current.pop();
            }
            return Some(current.clone());
        }
    }

    eval_atom(expr, vars)
}

/// Split a trailing `if (...)` or `unless (...)` off a statement.
///
/// Stock cows guard assignments this way: small.cow uses `unless ($eyes)` and
/// sus.cow uses `unless ($eyes ne 'oo')`.
fn split_condition(expr: &str) -> (&str, Option<(bool, String)>) {
    for (keyword, negate) in [(" unless (", true), (" if (", false)] {
        if let Some(position) = expr.rfind(keyword) {
            let rest = &expr[position + keyword.len()..];
            let condition = rest
                .trim_end()
                .trim_end_matches(';')
                .trim_end()
                .trim_end_matches(')')
                .trim();
            return (&expr[..position], Some((negate, condition.to_string())));
        }
    }
    (expr, None)
}

/// Evaluate a cow file condition: a variable, or a `eq`/`ne` comparison.
fn eval_condition(condition: &str, vars: &Vars) -> Option<bool> {
    let condition = condition.trim();
    let chars: Vec<char> = condition.chars().collect();
    let (name, end) = read_var_name(&chars, 1);
    let name = name?;
    let variable = vars.get(&name);
    let rest: String = chars[end..].iter().collect();
    let rest = rest.trim();

    for (operator, equals) in [("eq", true), ("ne", false)] {
        if let Some(operand) = rest.strip_prefix(operator) {
            let other = eval_atom(operand, vars)?;
            let same = variable.map(|v| *v == other).unwrap_or(false);
            return Some(if equals { same } else { !same });
        }
    }

    if rest.is_empty() {
        return Some(truthy(variable));
    }
    None
}

/// Perl truthiness for the values a cow file can hold.
fn truthy(value: Option<&String>) -> bool {
    match value {
        Some(value) => !value.is_empty() && value != "0",
        None => false,
    }
}

/// Parse `$balloon_lines[N] =~ tr/FROM/TO/`.
fn parse_translate(text: &str) -> Option<BalloonOp> {
    let open = text.find('[')?;
    let close = text[open..].find(']')? + open;
    let index: isize = text[open + 1..close].trim().parse().ok()?;

    let rest = &text[close + 1..];
    let operator = rest.find("=~")? + 2;
    let rest = rest[operator..].trim_start();
    let body = rest.strip_prefix("tr")?;

    let delimiter = body.chars().next()?;
    let mut groups: Vec<Vec<char>> = Vec::new();
    let mut current: Vec<char> = Vec::new();
    let mut escaped = false;
    for c in body[delimiter.len_utf8()..].chars() {
        if escaped {
            current.push(match c {
                't' => '\t',
                'n' => '\n',
                other => other,
            });
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == delimiter {
            groups.push(std::mem::take(&mut current));
            if groups.len() == 2 {
                break;
            }
            continue;
        }
        current.push(c);
    }
    if groups.len() != 2 {
        return None;
    }
    Some(BalloonOp::Translate {
        index,
        from: groups.remove(0),
        to: groups.remove(0),
    })
}

/// Normalise text inserted into the balloon: every newline starts a new line.
fn inserted_lines(text: &str) -> Vec<String> {
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n').map(str::to_string).collect()
}

/// Read a cow file with the given starting face values.
pub fn parse(text: &str, eyes: &str, tongue: &str, thoughts: &str) -> ParsedCow {
    let mut vars = Vars::new(eyes, tongue, thoughts);
    let mut cow = ParsedCow::default();
    let lines: Vec<String> = text.lines().map(str::to_string).collect();

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        // $the_cow = <<A . <<'B';  (one or more heredocs)
        if trimmed.starts_with("$the_cow") {
            let chars: Vec<char> = line.chars().collect();
            let mut cursor = 0usize;
            let mut body = String::new();
            let mut index = i + 1;
            let mut found = false;
            while let Some(offset) = find_from(&chars, cursor, '<') {
                let Some((term, interpolates, end)) = heredoc_opener(&chars, offset) else {
                    cursor = offset + 1;
                    continue;
                };
                let (raw, next) = heredoc_body(&lines, index, &term);
                let rendered = if interpolates {
                    interpolate(&raw, &vars)
                } else {
                    unescape_single(&raw)
                };
                body.push_str(&rendered);
                index = next;
                cursor = end;
                found = true;
            }
            if found {
                cow.template.push_str(&body);
                i = index;
                continue;
            }
            cow.warnings
                .push("cow file: $the_cow without a heredoc, ignored".to_string());
            i += 1;
            continue;
        }

        // unshift/push @balloon_lines, "..."
        if trimmed.starts_with("unshift") || trimmed.starts_with("push") {
            if let Some(at) = trimmed.find('@') {
                let rest = &trimmed[at..];
                if rest.starts_with("@balloon_lines") {
                    let front = trimmed.starts_with("unshift");
                    if let Some(comma) = trimmed.find(',') {
                        let value = &trimmed[comma + 1..];
                        let text = match take_quoted(value.trim()) {
                            Some((raw, single, _)) => {
                                if single {
                                    unescape_single(&raw)
                                } else {
                                    interpolate(&raw, &vars)
                                }
                            }
                            None => String::new(),
                        };
                        cow.ops.push(BalloonOp::Insert {
                            front,
                            lines: inserted_lines(&text),
                        });
                    }
                    i += 1;
                    continue;
                }
            }
        }

        // $balloon_lines[N] =~ tr/.../.../
        if trimmed.starts_with("$balloon_lines") {
            match parse_translate(trimmed) {
                Some(op) => cow.ops.push(op),
                None => cow
                    .warnings
                    .push(format!("cow file: unsupported balloon patch: {trimmed}")),
            }
            i += 1;
            continue;
        }

        // $name = expr; or $name .= expr;
        if trimmed.starts_with('$') {
            let chars: Vec<char> = trimmed.chars().collect();
            if let (Some(name), end) = read_var_name(&chars, 1) {
                let rest: String = chars[end..].iter().collect();
                let rest = rest.trim_start();
                let (append, tail) = if let Some(tail) = rest.strip_prefix(".=") {
                    (true, tail)
                } else if let Some(tail) = rest.strip_prefix('=') {
                    (false, tail)
                } else {
                    cow.warnings
                        .push(format!("cow file: unsupported statement: {trimmed}"));
                    i += 1;
                    continue;
                };
                let (value_expr, condition) = split_condition(tail);
                let should_run = match &condition {
                    None => true,
                    Some((negate, condition)) => {
                        let holds = eval_condition(condition, &vars).unwrap_or(false);
                        if *negate {
                            !holds
                        } else {
                            holds
                        }
                    }
                };
                if should_run {
                    match eval_expr(value_expr, &mut vars) {
                        Some(value) => {
                            if append {
                                vars.append(&name, &value);
                            } else {
                                vars.set(&name, value);
                            }
                        }
                        None => cow
                            .warnings
                            .push(format!("cow file: cannot evaluate: {trimmed}")),
                    }
                }
                i += 1;
                continue;
            }
        }

        cow.warnings
            .push(format!("cow file: unsupported statement: {trimmed}"));
        i += 1;
    }

    cow
}

fn find_from(chars: &[char], start: usize, needle: char) -> Option<usize> {
    (start..chars.len()).find(|i| chars[*i] == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_plain_template() {
        let cow = parse(
            "$the_cow = <<EOC;\n ($eyes) $tongue\n EOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, " (oo)   \n");
        assert!(cow.warnings.is_empty());
    }

    #[test]
    fn single_quoted_heredoc_keeps_backslashes() {
        let cow = parse(
            "$the_cow = <<'EOC';\n  \\\\ / \\\\\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "  \\ / \\\n");
    }

    #[test]
    fn concatenates_heredocs_in_order() {
        let cow = parse(
            "$the_cow = <<\"A\" . <<'B';\n  $thoughts\nA\n art \\\\ here\nB\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "  \\\n art \\ here\n");
    }

    #[test]
    fn applies_face_overrides() {
        let cow = parse(
            "$eyes = 'xx';\n$the_cow = <<EOC;\n($eyes)\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "(xx)\n");
    }

    #[test]
    fn chops_and_appends_like_udder_cow() {
        let cow = parse(
            "$other_eye = chop($eyes);\n$eyes .= \" $other_eye\";\n$the_cow = <<EOC;\n$eyes\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "o o\n");
    }

    #[test]
    fn honours_a_trailing_unless_guard() {
        let cow = parse(
            "$eyes = \"..\" unless ($eyes);\n$the_cow = <<EOC;\n($eyes)\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "(oo)\n");
    }

    #[test]
    fn guards_on_a_string_comparison() {
        // sus.cow blanks its eyes unless they still hold the default.
        let eyes = |given: &str| {
            parse(
                "$eyes = '  ' unless ($eyes ne 'oo');\n$the_cow = <<EOC;\n($eyes)\nEOC\n",
                given,
                "  ",
                "\\",
            )
            .template
        };
        assert_eq!(eyes("oo"), "(  )\n");
        assert_eq!(eyes("@@"), "(@@)\n");
    }

    #[test]
    fn repeats_strings_like_three_eyes_cow() {
        let cow = parse(
            "$extra = chop($eyes);\n$eyes .= ($extra x 2);\n$the_cow = <<EOC;\n($eyes)\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.template, "(ooo)\n");
    }

    #[test]
    fn patches_balloon_lines() {
        let cow = parse(
            "$balloon_lines[0] =~ tr/_/-/;\nunshift @balloon_lines, \"\\n\";\n$the_cow = <<EOC;\nx\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        let mut balloon = vec!["___".to_string(), " y ".to_string()];
        cow.patch_balloon(&mut balloon);
        assert_eq!(
            balloon,
            vec!["".to_string(), "---".to_string(), " y ".to_string()]
        );
    }

    #[test]
    fn reports_unknown_statements() {
        let cow = parse(
            "system('true');\n$the_cow = <<EOC;\nx\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert_eq!(cow.warnings.len(), 1);
        assert_eq!(cow.template, "x\n");
    }

    #[test]
    fn escapes_dollars_and_backslashes() {
        let cow = parse("$the_cow = <<EOC;\n\\$5 \\\\ ok\nEOC\n", "oo", "  ", "\\");
        assert_eq!(cow.template, "$5 \\ ok\n");
    }
}
