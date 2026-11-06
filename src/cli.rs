//! Command line parsing.
//!
//! The short options are the ones cowsay 3.8.4 accepts, including the `-L` and
//! `-N` no-ops, and unknown options produce cowsay's warning instead of an
//! error. `--help` and the long forms are additions.
//!
//! cowsay's `getopts` stops at the first non-option argument, so `cowsay hi -b`
//! speaks "hi -b" rather than turning bold. That behaviour is preserved.

use crate::balloon::FaceFlags;

/// When to enter the interactive type mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeMode {
    /// Type mode only when stdin is a terminal and no message was given.
    Auto,
    /// Never; behave exactly like cowsay.
    Never,
}

#[derive(Debug)]
pub struct Opts {
    pub flags: FaceFlags,
    pub eyes: Option<String>,
    pub tongue: Option<String>,
    pub cow: Option<String>,
    pub list: bool,
    pub random: bool,
    pub full_color: bool,
    pub no_wrap: bool,
    pub columns: usize,
    pub wrap_set: bool,
    pub type_mode: TypeMode,
    pub incremental: bool,
    pub append: bool,
    pub delay_ms: u64,
    pub words: Vec<String>,
    pub warnings: Vec<String>,
    pub help: bool,
    pub version: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            flags: FaceFlags::default(),
            eyes: None,
            tongue: None,
            cow: None,
            list: false,
            random: false,
            full_color: false,
            no_wrap: false,
            columns: 40,
            wrap_set: false,
            type_mode: TypeMode::Auto,
            incremental: false,
            append: false,
            delay_ms: 0,
            words: Vec::new(),
            warnings: Vec::new(),
            help: false,
            version: false,
        }
    }
}

impl Opts {
    fn apply_flag(&mut self, c: char) -> bool {
        match c {
            'b' => self.flags.borg = true,
            'd' => self.flags.dead = true,
            'g' => self.flags.greedy = true,
            'p' => self.flags.paranoid = true,
            's' => self.flags.stoned = true,
            't' => self.flags.tired = true,
            'w' => self.flags.wired = true,
            'y' => self.flags.young = true,
            'C' => self.full_color = true,
            'l' => self.list = true,
            'r' => self.random = true,
            'n' => self.no_wrap = true,
            'h' => self.help = true,
            // Accepted by cowsay and unused there too.
            'L' | 'N' => {}
            _ => return false,
        }
        true
    }

    fn apply_value(&mut self, c: char, value: &str) -> bool {
        match c {
            'e' => self.eyes = Some(value.to_string()),
            'T' => self.tongue = Some(value.to_string()),
            'f' => self.cow = Some(value.to_string()),
            'W' => {
                // cowsay feeds this to Perl arithmetic, where junk becomes 0 and
                // then spins forever. Clamping to 2 is the useful reading.
                self.columns = value.trim().parse::<usize>().unwrap_or(40).max(2);
                self.wrap_set = true;
            }
            _ => return false,
        }
        true
    }
}

pub fn parse(args: &[String]) -> Opts {
    let mut opts = Opts::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--" {
            opts.words.extend(args[i + 1..].iter().cloned());
            break;
        }

        if let Some(long) = arg.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };
            let missing = || value.clone().unwrap_or_default();
            match name {
                "help" => opts.help = true,
                "version" => opts.version = true,
                "list" => opts.list = true,
                "random" => opts.random = true,
                "color" | "colour" => opts.full_color = true,
                "no-wrap" => opts.no_wrap = true,
                "no-type" => opts.type_mode = TypeMode::Never,
                "incremental" => opts.incremental = true,
                "append" => opts.append = true,
                "eyes" => opts.eyes = Some(missing()),
                "tongue" => opts.tongue = Some(missing()),
                "cow" => opts.cow = Some(missing()),
                "wrap" => {
                    opts.columns = missing().parse::<usize>().unwrap_or(40).max(2);
                    opts.wrap_set = true;
                }
                "delay" => opts.delay_ms = missing().parse::<u64>().unwrap_or(0),
                _ => opts.warnings.push(format!("Unknown option: {name}")),
            }
            i += 1;
            continue;
        }

        if arg.len() > 1 && arg.starts_with('-') {
            let chars: Vec<char> = arg.chars().skip(1).collect();
            let mut k = 0;
            while k < chars.len() {
                let c = chars[k];
                if matches!(c, 'e' | 'T' | 'f' | 'W') {
                    let attached: String = chars[k + 1..].iter().collect();
                    let value = if !attached.is_empty() {
                        attached
                    } else {
                        i += 1;
                        args.get(i).cloned().unwrap_or_default()
                    };
                    opts.apply_value(c, &value);
                    k = chars.len();
                    continue;
                }
                if !opts.apply_flag(c) {
                    opts.warnings.push(format!("Unknown option: {c}"));
                }
                k += 1;
            }
            i += 1;
            continue;
        }

        // First non-option argument starts the message; everything after it is
        // message text, options included.
        opts.words.extend(args[i..].iter().cloned());
        break;
    }

    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_cowsay_options() {
        let opts = parse(&args(&["-bd", "-f", "tux", "-W", "20", "hi"]));
        assert!(opts.flags.borg && opts.flags.dead);
        assert_eq!(opts.cow.as_deref(), Some("tux"));
        assert_eq!(opts.columns, 20);
        assert_eq!(opts.words, args(&["hi"]));
    }

    #[test]
    fn stops_option_parsing_at_the_message() {
        let opts = parse(&args(&["hi", "-b"]));
        assert!(!opts.flags.borg);
        assert_eq!(opts.words, args(&["hi", "-b"]));
    }

    #[test]
    fn accepts_attached_option_values() {
        let opts = parse(&args(&["-eXX", "-W10", "hi"]));
        assert_eq!(opts.eyes.as_deref(), Some("XX"));
        assert_eq!(opts.columns, 10);
    }

    #[test]
    fn warns_about_unknown_options_like_cowsay() {
        let opts = parse(&args(&["-Z", "hi"]));
        assert_eq!(opts.warnings, vec!["Unknown option: Z".to_string()]);
        assert_eq!(opts.words, args(&["hi"]));
    }

    #[test]
    fn parses_cowtype_options() {
        let opts = parse(&args(&["--no-type", "--delay=50"]));
        assert_eq!(opts.type_mode, TypeMode::Never);
        assert_eq!(opts.delay_ms, 50);
    }
}
