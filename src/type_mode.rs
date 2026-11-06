//! The type mode.
//!
//! With no message and a terminal on stdin, cowtype becomes a small editor: it
//! turns local echo off and draws whatever you type *inside the balloon*, so an
//! asciinema recording shows the text assembling itself in the cow's mouth
//! instead of a bare prompt. Press Enter for a new balloon line, Ctrl-D to
//! finish, Ctrl-C to abort.
//!
//! `--incremental` brings the same progressive rendering to piped input, which
//! is handy for animating a file into a recording.

use std::io::{self, Read, Write};

/// Height of the frame currently on screen, so it can be redrawn in place.
struct Screen {
    height: usize,
    append: bool,
}

impl Screen {
    fn new(append: bool) -> Self {
        Screen { height: 0, append }
    }

    /// Draw `frame`, replacing the previous one unless appending.
    fn draw(&mut self, frame: &str, out: &mut impl Write) -> io::Result<()> {
        if self.height > 0 {
            if self.append {
                out.write_all(b"\n")?;
            } else {
                // Up to the first line of the old frame, then erase downwards.
                write!(out, "\x1b[{}A\r\x1b[J", self.height)?;
            }
        }
        out.write_all(frame.as_bytes())?;
        if !frame.ends_with('\n') {
            out.write_all(b"\n")?;
        }
        out.flush()?;
        self.height = frame.lines().count();
        Ok(())
    }

    /// Ctrl-L: start over from a blank screen.
    fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        out.write_all(b"\x1b[H\x1b[2J")?;
        out.flush()?;
        self.height = 0;
        Ok(())
    }
}

/// A terminal in character-at-a-time, no-echo mode.
struct RawMode {
    saved: libc::termios,
    restored: bool,
}

/// Saved settings for the signal handler, which cannot call into Rust safely.
static mut SAVED_TERMIOS: Option<libc::termios> = None;

extern "C" fn restore_and_exit(signal: libc::c_int) {
    unsafe {
        if let Some(saved) = (*std::ptr::addr_of!(SAVED_TERMIOS)).as_ref() {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, saved);
        }
        libc::write(libc::STDOUT_FILENO, b"\n".as_ptr().cast(), 1);
        libc::_exit(128 + signal);
    }
}

fn install_signal_handlers() {
    unsafe {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            libc::signal(signal, restore_and_exit as libc::sighandler_t);
        }
    }
}

impl RawMode {
    fn enter() -> Option<Self> {
        unsafe {
            let mut settings: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut settings) != 0 {
                return None;
            }
            let saved = settings;

            // No line editing, no echo, no keyboard signals: this module owns
            // every byte that arrives.
            settings.c_lflag &=
                !(libc::ICANON | libc::ECHO | libc::ECHONL | libc::ISIG | libc::IEXTEN);
            settings.c_iflag &= !(libc::IXON | libc::ICRNL | libc::INLCR | libc::ISTRIP);
            settings.c_cc[libc::VMIN] = 1;
            settings.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSADRAIN, &settings) != 0 {
                return None;
            }
            SAVED_TERMIOS = Some(saved);
            install_signal_handlers();

            let mut out = io::stdout();
            let _ = out.write_all(b"\x1b[?25l");
            let _ = out.flush();
            Some(RawMode {
                saved,
                restored: false,
            })
        }
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.saved);
        }
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[?25h");
        let _ = out.flush();
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

pub fn stdin_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

pub fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

/// Terminal width in columns, when it can be read.
pub fn screen_width() -> Option<usize> {
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0 && size.ws_col > 0 {
            Some(size.ws_col as usize)
        } else {
            None
        }
    }
}

fn read_byte() -> Option<u8> {
    let mut byte = 0u8;
    let read = unsafe {
        libc::read(
            libc::STDIN_FILENO,
            &mut byte as *mut u8 as *mut libc::c_void,
            1,
        )
    };
    if read == 1 {
        Some(byte)
    } else {
        None
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Read one character, pulling continuation bytes for multibyte input.
fn read_char(first: u8) -> Option<char> {
    let mut bytes = vec![first];
    for _ in 1..utf8_len(first) {
        bytes.push(read_byte()?);
    }
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.chars().next())
}

/// Swallow a control sequence introduced by ESC, so arrow keys do nothing.
///
/// Returns the byte to reprocess when the sequence was not a control sequence
/// after all, so that pressing ESC on its own does not swallow the next key.
fn skip_escape() -> Option<u8> {
    let second = read_byte()?;
    if second == b'[' || second == b'O' {
        while let Some(byte) = read_byte() {
            if (0x40..=0x7e).contains(&byte) {
                break;
            }
        }
        return None;
    }
    if second == 0x1b {
        return None;
    }
    Some(second)
}

fn delete_word(line: &mut String) {
    while line.ends_with(' ') {
        line.pop();
    }
    while let Some(last) = line.chars().last() {
        if last == ' ' {
            break;
        }
        line.pop();
    }
}

/// Interactive type mode. `render` turns the committed lines plus the line in
/// progress into a frame.
pub fn interactive<F>(render: F, append: bool) -> io::Result<i32>
where
    F: Fn(&[String], Option<&str>) -> String,
{
    let mut raw = match RawMode::enter() {
        Some(raw) => raw,
        None => {
            eprintln!("cowtype: cannot switch the terminal into type mode");
            return Ok(1);
        }
    };

    let mut out = io::stdout();
    let mut screen = Screen::new(append);
    let mut committed: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut code = 0;

    screen.draw(&render(&committed, Some(&current)), &mut out)?;

    let mut pending: Option<u8> = None;
    'input: loop {
        let byte = match pending.take() {
            Some(byte) => byte,
            None => match read_byte() {
                Some(byte) => byte,
                None => break,
            },
        };
        match byte {
            0x03 => {
                // Ctrl-C: abort without the final frame.
                code = 130;
                break 'input;
            }
            0x04 => {
                // Ctrl-D: finish, keeping the line that was being typed.
                break 'input;
            }
            b'\r' | b'\n' => {
                committed.push(std::mem::take(&mut current));
            }
            0x7f | 0x08 => {
                current.pop();
            }
            0x15 => current.clear(),
            0x17 => delete_word(&mut current),
            0x0c => {
                screen.clear(&mut out)?;
                screen.draw(&render(&committed, Some(&current)), &mut out)?;
                continue;
            }
            0x1b => {
                pending = skip_escape();
                continue;
            }
            0x00..=0x1f => continue,
            other => match read_char(other) {
                Some(c) => current.push(c),
                None => break,
            },
        }
        screen.draw(&render(&committed, Some(&current)), &mut out)?;
    }

    if !current.is_empty() {
        committed.push(current);
    }
    screen.draw(&render(&committed, None), &mut out)?;
    out.write_all(b"\n")?;
    out.flush()?;
    raw.restore();
    Ok(code)
}

/// Progressive rendering for piped input: one frame per line, appended.
pub fn stream<F>(render: F, delay_ms: u64) -> io::Result<i32>
where
    F: Fn(&[String], Option<&str>) -> String,
{
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let mut out = io::stdout();
    let mut screen = Screen::new(true);
    let mut committed: Vec<String> = Vec::new();

    let mut lines: Vec<&str> = input.split('\n').collect();
    if input.ends_with('\n') {
        lines.pop();
    }

    for line in lines {
        committed.push(line.to_string());
        screen.draw(&render(&committed, Some("")), &mut out)?;
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    }

    // The last frame drops the empty line that was waiting for input.
    screen.draw(&render(&committed, None), &mut out)?;
    Ok(0)
}
