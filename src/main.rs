//! cowtype: a cowsay-compatible speaker with a type mode.
//!
//! Rendering, options, and cow files follow cowsay 3.8.4. The addition is the
//! type mode (see [`type_mode`]): with no message and a terminal on stdin, what
//! you type is drawn inside the balloon, which makes input stand out in an
//! asciinema recording.

mod balloon;
mod cli;
mod cow;
mod text;
mod type_mode;

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The cowsay release this program reproduces.
const COMPAT: &str = "3.8.4";

/// Everything needed to turn message lines into a complete cow picture.
struct Renderer {
    art: cow::ParsedCow,
    columns: usize,
    think: bool,
    no_wrap: bool,
    /// Type mode keeps every typed line on its own balloon line: code should
    /// not be re-flowed, and an empty line is a line you are about to fill.
    preserve_lines: bool,
}

impl Renderer {
    /// Turn the input lines into balloon lines.
    fn message(&self, raw: &[String]) -> Vec<String> {
        if self.preserve_lines {
            let mut lines = Vec::new();
            for line in raw {
                if line.is_empty() {
                    lines.push(String::new());
                    continue;
                }
                let wrapped = text::fill(std::slice::from_ref(line), self.columns);
                if wrapped.is_empty() {
                    // A line of nothing but spaces still occupies a balloon line.
                    lines.push(text::expand(line));
                } else {
                    lines.extend(wrapped);
                }
            }
            lines
        } else if self.no_wrap {
            raw.iter().map(|line| text::expand(line)).collect()
        } else {
            text::fill(raw, self.columns)
        }
    }

    /// Balloon plus cow art, ending exactly where cowsay ends.
    fn frame(&self, raw: &[String]) -> String {
        let message = self.message(raw);

        let mut balloon = balloon::lines(&message, self.think);
        self.art.patch_balloon(&mut balloon);

        let mut out = String::new();
        for line in &balloon {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&self.art.template);
        out
    }
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_else(|| "cowtype".to_string());
    let prog = Path::new(&argv0)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cowtype".to_string());
    // cowsay decides between saying and thinking by looking at its own name, so
    // a `cowthink` symlink keeps working here too.
    let think = prog.to_lowercase().contains("think");

    let opts = cli::parse(&args.collect::<Vec<String>>());
    for warning in &opts.warnings {
        eprintln!("{warning}");
    }

    if opts.help {
        print!("{}", help(&prog));
        return 0;
    }
    if opts.version {
        println!("{prog} version {VERSION} (compatible with cowsay {COMPAT})");
        return 0;
    }

    let cowpath = cow::cowpath();

    if opts.list {
        let pretty = type_mode::stdout_is_tty();
        print!("{}", cow::render_list(&cowpath, opts.columns, pretty));
        return 0;
    }

    if opts.no_wrap && !opts.words.is_empty() {
        // cowsay only supports -n for input read from stdin.
        print!("{}", help(&prog));
        return 1;
    }

    let cow_name = if opts.random {
        match cow::random_name(&cowpath, opts.full_color) {
            Some(name) => name,
            None => {
                eprintln!("{prog}: no cows found on the cowpath");
                return 2;
            }
        }
    } else {
        opts.cow
            .clone()
            .unwrap_or_else(|| "default.cow".to_string())
    };

    let Some(file) = cow::resolve(&cow_name, &cowpath) else {
        eprintln!("{prog}: Could not find cowfile for '{cow_name}'!");
        return 2;
    };
    let source = match fs::read_to_string(&file) {
        Ok(source) => source,
        Err(error) => {
            eprintln!(
                "{prog}: Error reading cow definition from {}: {error}",
                file.display()
            );
            return 2;
        }
    };

    let eyes = opts.eyes.clone().unwrap_or_else(|| "oo".to_string());
    let tongue = opts.tongue.clone().unwrap_or_else(|| "  ".to_string());
    let mut face = balloon::Face::new(&first_two(&eyes), &first_two(&tongue));
    face.apply_flags(&opts.flags);

    let art = cow::parse(&source, &face.eyes, &face.tongue, balloon::thoughts(think));
    for warning in &art.warnings {
        eprintln!("{prog}: {warning}");
    }

    let type_mode_wanted =
        opts.words.is_empty() && opts.type_mode == cli::TypeMode::Auto && type_mode::stdin_is_tty();

    // Typing code wants a wider balloon than cowsay's 40 columns, so type mode
    // defaults to the terminal width unless -W says otherwise.
    let columns = if type_mode_wanted && !opts.wrap_set {
        type_mode::screen_width()
            .map(|width| width.saturating_sub(4))
            .filter(|width| *width >= 20)
            .unwrap_or(opts.columns)
    } else {
        opts.columns
    };

    let renderer = Renderer {
        art,
        columns,
        think,
        no_wrap: opts.no_wrap,
        preserve_lines: type_mode_wanted || (opts.incremental && opts.words.is_empty()),
    };

    if type_mode_wanted {
        let append = opts.append;
        let result = type_mode::interactive(
            |raw, current| {
                let mut message = raw.to_vec();
                if let Some(current) = current {
                    message.push(current.to_string());
                }
                renderer.frame(&message)
            },
            append,
        );
        return match result {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{prog}: {error}");
                1
            }
        };
    }

    if opts.incremental && opts.words.is_empty() {
        let delay = opts.delay_ms;
        let result = type_mode::stream(
            |raw, current| {
                let mut message = raw.to_vec();
                if let Some(current) = current {
                    message.push(current.to_string());
                }
                renderer.frame(&message)
            },
            delay,
        );
        return match result {
            Ok(code) => code,
            Err(error) => {
                eprintln!("{prog}: {error}");
                1
            }
        };
    }

    let raw = if opts.words.is_empty() {
        read_stdin()
    } else {
        vec![opts.words.join(" ")]
    };

    let mut stdout = io::stdout();
    let out = renderer.frame(&raw);
    let _ = stdout.write_all(out.as_bytes());
    let _ = stdout.flush();
    0
}

/// cowsay keeps only the first two characters of `-e` and `-T`.
fn first_two(value: &str) -> String {
    value.chars().take(2).collect()
}

/// Read stdin the way cowsay's `chomp(@message = <STDIN>)` does.
fn read_stdin() -> Vec<String> {
    let mut bytes = Vec::new();
    let _ = io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn help(prog: &str) -> String {
    format!(
        "\
{prog} version {VERSION} (compatible with cowsay {COMPAT})

Usage:

    {prog} [-bdgpstwy] [-f <cowfile>] [-r [-C]] [-e <eyes>] [-T <tongue>]
        [-W <wrapcolumn>] [-n]
        <message>

    {prog} -l              # List defined cows
    {prog} [-h | --help]   # Display this help screen

Options:

    -b, -d, -g, -s, -t, -w, and -y activate Borg, dead, greedy, sleepy, tired,
        wired, and young appearance modes, respectively.

    -f <cowfile> selects an alternate cow picture. <cowfile> may be either the
        name of a cow defined in a cowdir on the cowpath (without the '.cow'
        file extension), or the path to a cowfile (with the '.cow' extension).
        `{prog} -l` will list the names of available cows.

    -r selects a random cowfile from those present on the cowpath. If -C is
        also given, then full-color cows will be included.

    -e <eyes> defines a custom eye appearance, using the first two characters.

    -T <tongue> defines a custom tongue appearance, using the first two
        characters.

    -n activates word wrapping, to support messages with arbitrary whitespace.
        It only applies to input read from stdin.

    -W <wrapcolumn> controls where line wrapping occurs. Default is 40 columns.

Type mode:

    With no message and a terminal on stdin, {prog} accepts typing and draws it
    inside the balloon, which makes input stand out in an asciinema recording.
    Enter starts a new balloon line, Ctrl-D finishes, Ctrl-C aborts, Ctrl-U
    clears the line, Ctrl-W deletes a word, Ctrl-L redraws.

    --no-type        never enter type mode (behave exactly like cowsay)
    --incremental    render one frame per line for piped input
    --append         add frames instead of redrawing in place
    --delay <ms>     pause between incremental frames
    --version        print the version
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer(columns: usize, no_wrap: bool) -> Renderer {
        let art = cow::parse(
            "$the_cow = <<EOC;\n        $thoughts   ^__^\n         $thoughts  ($eyes)\\\\\nEOC\n",
            "oo",
            "  ",
            "\\",
        );
        assert!(art.warnings.is_empty());
        Renderer {
            art,
            columns,
            think: false,
            no_wrap,
            preserve_lines: false,
        }
    }

    #[test]
    fn matches_cowsay_for_a_single_line() {
        let out = renderer(40, false).frame(&["hello world".to_string()]);
        assert_eq!(
            out,
            " _____________\n< hello world >\n -------------\n        \\   ^__^\n         \\  (oo)\\\n"
        );
    }

    #[test]
    fn keeps_whitespace_when_no_wrap_is_on() {
        let out = renderer(40, true).frame(&["a   b\tc".to_string()]);
        assert!(out.contains("< a   b   c >"), "{out}");
    }
}
