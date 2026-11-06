//! The speech balloon: borders, padding, and the face that the cow shows.
//!
//! Mirrors `construct_balloon` and `construct_face` from cowsay 3.8.4.

use crate::text;

/// Eyes and tongue, the two parts of the face a cow file can draw.
pub struct Face {
    pub eyes: String,
    pub tongue: String,
}

impl Face {
    pub fn new(eyes: &str, tongue: &str) -> Self {
        Face {
            eyes: eyes.to_string(),
            tongue: tongue.to_string(),
        }
    }

    /// Apply the `-b -d -g -p -s -t -w -y` appearance flags, in cowsay's order
    /// (later flags win).
    pub fn apply_flags(&mut self, flags: &FaceFlags) {
        if flags.borg {
            self.eyes = "==".into();
        }
        if flags.dead {
            self.eyes = "xx".into();
            self.tongue = "U ".into();
        }
        if flags.greedy {
            self.eyes = "$$".into();
        }
        if flags.paranoid {
            self.eyes = "@@".into();
        }
        if flags.stoned {
            self.eyes = "**".into();
            self.tongue = "U ".into();
        }
        if flags.tired {
            self.eyes = "--".into();
        }
        if flags.wired {
            self.eyes = "OO".into();
        }
        if flags.young {
            self.eyes = "..".into();
        }
    }
}

/// Appearance flags shared by the CLI and the face.
#[derive(Debug, Default, Clone, Copy)]
pub struct FaceFlags {
    pub borg: bool,
    pub dead: bool,
    pub greedy: bool,
    pub paranoid: bool,
    pub stoned: bool,
    pub tired: bool,
    pub wired: bool,
    pub young: bool,
}

/// The character that points from the balloon to the cow.
pub fn thoughts(think: bool) -> &'static str {
    if think {
        "o"
    } else {
        "\\"
    }
}

/// Build the balloon lines, without trailing newlines.
///
/// `cowsay` indexes `$message[0]` even when the message is empty, so an empty
/// message still produces a two-column balloon.
pub fn lines(message: &[String], think: bool) -> Vec<String> {
    let widest = message.iter().map(|l| text::width(l)).max().unwrap_or(0);
    let inner = widest + 2;

    let (left_top, right_top, left_bottom, right_bottom, left, right) = if think {
        ("(", ")", "(", ")", "(", ")")
    } else if message.len() < 2 {
        ("<", ">", "", "", "", "")
    } else {
        ("/", "\\", "\\", "/", "|", "|")
    };

    let pad = |content: &str| {
        let w = text::width(content);
        let mut s = String::with_capacity(content.len() + widest.saturating_sub(w));
        s.push_str(content);
        s.extend(std::iter::repeat(' ').take(widest.saturating_sub(w)));
        s
    };

    let mut out = Vec::with_capacity(message.len() + 2);
    out.push(format!(" {}", "_".repeat(inner)));

    match message.len() {
        0 => out.push(format!("< {} >", pad(""))),
        1 => out.push(format!("{} {} {}", left_top, pad(&message[0]), right_top)),
        n => {
            out.push(format!("{} {} {}", left_top, pad(&message[0]), right_top));
            for line in &message[1..n - 1] {
                out.push(format!("{} {} {}", left, pad(line), right));
            }
            out.push(format!(
                "{} {} {}",
                left_bottom,
                pad(&message[n - 1]),
                right_bottom
            ));
        }
    }

    out.push(format!(" {}", "-".repeat(inner)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(message: &[&str], think: bool) -> String {
        let owned: Vec<String> = message.iter().map(|s| s.to_string()).collect();
        lines(&owned, think).join("\n")
    }

    // Expected strings below were captured from cowsay 3.8.4.

    #[test]
    fn single_line_balloon() {
        assert_eq!(
            render(&["hello world"], false),
            " _____________\n< hello world >\n -------------"
        );
    }

    #[test]
    fn empty_message_balloon() {
        assert_eq!(render(&[], false), " __\n<  >\n --");
    }

    #[test]
    fn multi_line_balloon() {
        assert_eq!(
            render(&["one", "", "two"], false),
            " _____\n/ one \\\n|     |\n\\ two /\n -----"
        );
    }

    #[test]
    fn think_balloon_uses_parentheses() {
        assert_eq!(render(&["hi"], true), " ____\n( hi )\n ----");
    }

    #[test]
    fn pads_short_lines_by_display_width() {
        // "中文" is four columns wide, "abcdef" is six, so it takes two spaces.
        assert_eq!(
            render(&["中文", "abcdef"], false),
            " ________\n/ 中文   \\\n\\ abcdef /\n --------"
        );
    }
}
