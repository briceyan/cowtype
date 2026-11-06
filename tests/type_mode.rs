//! PTY-driven tests for the type mode.
//!
//! Type mode only starts when stdin is a terminal, so these tests give the
//! binary a real pseudo-terminal and type into it. The assertions look at the
//! bytes the child writes, which is also what an asciinema recording captures.

#![cfg(unix)]

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const OUR_BIN: &str = env!("CARGO_BIN_EXE_cowtype");
const TIMEOUT: Duration = Duration::from_secs(10);

fn duplicate(fd: RawFd) -> File {
    let copy = unsafe { libc::dup(fd) };
    assert!(copy >= 0, "dup failed");
    unsafe { File::from_raw_fd(copy) }
}

/// One cowtype process on a pseudo-terminal, plus everything it has written.
struct Session {
    child: Child,
    master: File,
    output: String,
    frames: Receiver<Vec<u8>>,
    reaped: bool,
}

impl Session {
    fn start(args: &[&str], columns: u16) -> Session {
        unsafe {
            let mut master: libc::c_int = -1;
            let mut slave: libc::c_int = -1;
            let opened = libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_eq!(opened, 0, "openpty failed");

            let size = libc::winsize {
                ws_row: 24,
                ws_col: columns,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            libc::ioctl(master, libc::TIOCSWINSZ, &size);

            let child = Command::new(OUR_BIN)
                .args(args)
                .stdin(Stdio::from(duplicate(slave)))
                .stdout(Stdio::from(duplicate(slave)))
                .stderr(Stdio::from(duplicate(slave)))
                .spawn()
                .expect("failed to start cowtype");
            libc::close(slave);

            let master = File::from_raw_fd(master);
            let mut reader = master.try_clone().expect("cannot clone the pty");
            let (sender, frames) = mpsc::channel();
            thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if sender.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });

            Session {
                child,
                master,
                output: String::new(),
                frames,
                reaped: false,
            }
        }
    }

    /// Type into the child, as a person at the keyboard would.
    fn send(&mut self, bytes: &[u8]) {
        self.master
            .write_all(bytes)
            .expect("cannot write to the pty");
        self.master.flush().ok();
    }

    /// Wait until the child has written `needle` somewhere.
    fn expect(&mut self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.output.contains(needle) {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .frames
                .recv_timeout(remaining.max(Duration::from_millis(1)))
            {
                Ok(chunk) => self.output.push_str(&String::from_utf8_lossy(&chunk)),
                Err(RecvTimeoutError::Timeout) => panic!(
                    "timed out waiting for {needle:?}; cowtype wrote {:?}",
                    self.output
                ),
                Err(RecvTimeoutError::Disconnected) => panic!(
                    "cowtype exited before writing {needle:?}; it wrote {:?}",
                    self.output
                ),
            }
        }
    }

    /// Ctrl-D, then the exit status and complete output.
    fn finish(mut self) -> (i32, String) {
        self.send(&[0x04]);
        let status = self.child.wait().expect("cowtype did not exit");
        self.reaped = true;
        // Collect whatever the reader thread still holds.
        while let Ok(chunk) = self.frames.recv_timeout(Duration::from_millis(200)) {
            self.output.push_str(&String::from_utf8_lossy(&chunk));
        }
        (
            status.code().unwrap_or(-1),
            std::mem::take(&mut self.output),
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[test]
fn typed_text_grows_the_balloon_in_place() {
    let mut session = Session::start(&[], 60);
    session.expect("^__^");
    session.send(b"hi");
    session.expect("< hi >");
    session.send(b"\r");
    session.send(b"yo");
    session.expect("yo");

    let (code, output) = session.finish();
    assert_eq!(
        code, 0,
        "type mode should exit cleanly on Ctrl-D: {output:?}"
    );
    // The cursor is hidden for the session and restored at the end, and every
    // keystroke rewinds over the previous frame instead of scrolling.
    assert!(
        output.contains("\x1b[?25l"),
        "cursor should be hidden: {output:?}"
    );
    assert!(
        output.contains("\x1b[?25h"),
        "cursor should be restored: {output:?}"
    );
    assert!(
        output.contains("\x1b["),
        "frames should redraw in place: {output:?}"
    );
    assert!(
        output.contains("< hi >"),
        "the first line should stay: {output:?}"
    );
    assert!(
        output.contains("yo"),
        "the second line should stay: {output:?}"
    );
}

#[test]
fn backspace_edits_the_balloon_line() {
    let mut session = Session::start(&[], 60);
    session.expect("^__^");
    session.send(b"ab");
    session.expect("< ab >");
    session.send(&[0x7f]);
    session.expect("< a >");

    let (code, _) = session.finish();
    assert_eq!(code, 0);
}

#[test]
fn ctrl_c_aborts_with_status_130() {
    let mut session = Session::start(&[], 60);
    session.expect("^__^");
    session.send(&[0x03]);
    let status = session.child.wait().expect("cowtype did not exit");
    session.reaped = true;
    assert_eq!(status.code(), Some(130));
}

#[test]
fn wide_text_keeps_the_balloon_square() {
    let mut session = Session::start(&[], 60);
    session.expect("^__^");
    session.send("中文".as_bytes());
    // "中文" is four columns, so the top border grows to six underscores.
    session.expect(" ______");

    let (code, _) = session.finish();
    assert_eq!(code, 0);
}

#[test]
fn piped_input_skips_type_mode() {
    let mut child = Command::new(OUR_BIN)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start cowtype");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"hi\n")
        .expect("cannot write stdin");
    let output = child.wait_with_output().expect("cowtype did not exit");
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        text.contains("< hi >"),
        "piped input should be spoken: {text:?}"
    );
    assert!(
        !text.contains('\x1b'),
        "piped input must not redraw: {text:?}"
    );
}
