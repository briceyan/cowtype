//! Differential tests against the Perl cowsay.
//!
//! These run the real `cowsay` binary and require our output to match byte for
//! byte. They are skipped when cowsay is not installed, so `cargo test` works on
//! a machine without it.
//!
//! Two differences are deliberate and therefore not compared here:
//!
//! * widths use terminal columns, so CJK text lines up and `cowsay`'s
//!   byte-counted balloon does not;
//! * `-W` below 2 is clamped instead of producing cowsay's nonsense output.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const OUR_BIN: &str = env!("CARGO_BIN_EXE_cowtype");

/// Where cowsay keeps its cow files on the platforms we care about.
fn cow_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("COWPATH") {
        return std::env::split_paths(&path).next();
    }
    let mut candidates: Vec<PathBuf> = vec![
        "/usr/share/cowsay/cows".into(),
        "/usr/local/share/cowsay/cows".into(),
        "/usr/local/share/cows".into(),
        "/opt/homebrew/share/cowsay/cows".into(),
    ];
    // Homebrew keeps Cellar versions out of the shared prefix on some installs.
    if let Ok(entries) = std::fs::read_dir("/opt/homebrew/Cellar/cowsay") {
        for entry in entries.flatten() {
            candidates.push(entry.path().join("share/cowsay/cows"));
        }
    }
    candidates.into_iter().find(|dir| dir.is_dir())
}

fn cowsay_works() -> bool {
    Command::new("cowsay")
        .arg("probe")
        .stdin(Stdio::null())
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

fn run(program: &Path, args: &[&str], stdin: &[u8], cowpath: Option<&Path>) -> Vec<u8> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cowpath) = cowpath {
        command.env("COWPATH", cowpath);
    }
    let mut child = command.spawn().expect("failed to spawn");
    {
        let mut pipe = child.stdin.take().expect("no stdin");
        pipe.write_all(stdin).expect("failed to write stdin");
    }
    child.wait_with_output().expect("failed to wait").stdout
}

fn compare(args: &[&str], stdin: &[u8]) {
    compare_programs(Path::new(OUR_BIN), Path::new("cowsay"), args, stdin);
}

/// Compare two programs byte for byte with the same environment.
fn compare_programs(ours: &Path, theirs: &Path, args: &[&str], stdin: &[u8]) {
    let cowpath = cow_dir();
    let want = run(theirs, args, stdin, cowpath.as_deref());
    let got = run(ours, args, stdin, cowpath.as_deref());
    assert_eq!(
        String::from_utf8_lossy(&want),
        String::from_utf8_lossy(&got),
        "difference for {theirs:?} against {ours:?} with args {args:?} and stdin {stdin:?}"
    );
}

fn program_works(name: &str) -> bool {
    Command::new(name)
        .arg("probe")
        .stdin(Stdio::null())
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

#[test]
fn matches_cowsay_for_messages() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    let fifty = "a".repeat(50);
    let thirty_nine = "b".repeat(39);
    let forty = "c".repeat(40);
    let cases: &[(&[&str], &[u8])] = &[
        (&["hello world"], b""),
        (&[""], b""),
        (&["a"], b""),
        (&[" "], b""),
        (&["  "], b""),
        (&["a\tb\tc"], b""),
        (&["a    b"], b""),
        (&["  indented text"], b""),
        (&[fifty.as_str()], b""),
        (&[thirty_nine.as_str()], b""),
        (&[forty.as_str()], b""),
        (
            &["the quick brown fox jumps over the lazy dog and keeps running far away"],
            b"",
        ),
        (&["-W", "10", "aaa bbb ccc ddd"], b""),
        (&["-W", "5", "aaa   bbb"], b""),
        (&["-W", "2", "abc def"], b""),
        (&["-e", "XYZ", "-T", "ABC", "hi"], b""),
        (&["-e", "Q", "hi"], b""),
        (&["-bd", "hi"], b""),
        (&["-g", "hi"], b""),
        (&["-p", "hi"], b""),
        (&["-s", "hi"], b""),
        (&["-t", "hi"], b""),
        (&["-w", "hi"], b""),
        (&["-y", "hi"], b""),
        (&["-N", "hi"], b""),
        (&["-L", "hi"], b""),
        (&["a", "b", "c"], b""),
    ];
    for (args, stdin) in cases {
        compare(args, stdin);
    }
}

#[test]
fn matches_cowsay_for_stdin() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    let cases: &[(&[&str], &[u8])] = &[
        (&[], b"one\ntwo\nthree\n"),
        (&[], b"one\n\ntwo\n"),
        (&[], b"one\n\n\n\ntwo\n"),
        (&[], b"\n"),
        (&[], b"\n\n"),
        (&[], b""),
        (&[], b"  indented\n    more\n"),
        (&[], b"a   b\tc\n"),
        (&[], b"no trailing newline"),
        (&[], b"mixed \t tabs and   spaces here\n"),
        (&["-n"], b"a   b\tc\n"),
        (&["-n"], b"  x   y\n"),
        (&["-W", "12"], b"alpha beta gamma delta\n"),
        (&["-W", "12"], b"alpha\n\nbeta gamma delta epsilon\n"),
    ];
    for (args, stdin) in cases {
        compare(args, stdin);
    }
}

#[test]
fn matches_cowsay_for_every_installed_cow() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    let Some(dir) = cow_dir() else {
        eprintln!("no cow directory found, skipping");
        return;
    };
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir)
        .expect("cannot read cow dir")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("cow") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        compare(&["-f", &name, "hello there"], b"");
        checked += 1;
    }
    assert!(checked > 0, "no cow files found in {}", dir.display());
}

#[test]
fn listing_matches_cowsay() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    // Piped stdout makes `-l` print one name per line.
    compare(&["-l"], b"");
}

/// cowsay picks "say" or "think" from argv0, and so does cowtype, so the test
/// runs our binary through a `cowthink` symlink.
#[test]
fn matches_cowthink_through_argv0() {
    if !program_works("cowthink") {
        eprintln!("cowthink not installed, skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("cowtype-argv0-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("cannot create a temporary directory");
    let link = dir.join("cowthink");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(OUR_BIN, &link).expect("cannot create the cowthink symlink");

    for args in [
        &["hi there"][..],
        &["-e", "XX", "-T", "UU", "hi"][..],
        &["-W", "12", "a thought that needs wrapping"][..],
        &["-f", "tux", "hi"][..],
    ] {
        compare_programs(&link, Path::new("cowthink"), args, b"");
    }

    std::fs::remove_file(&link).ok();
    std::fs::remove_dir(&dir).ok();
}

/// `-r` has to land on a real cow, so every picture it prints must be one the
/// same build prints for an explicit `-f`.
#[test]
fn random_mode_speaks_with_an_installed_cow() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    let Some(dir) = cow_dir() else {
        eprintln!("no cow directory found, skipping");
        return;
    };
    let mut spoken: HashSet<Vec<u8>> = HashSet::new();
    for entry in std::fs::read_dir(&dir)
        .expect("cannot read cow dir")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("cow") {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        spoken.insert(run(
            Path::new(OUR_BIN),
            &["-f", &name, "hello there"],
            b"",
            Some(&dir),
        ));
    }
    assert!(
        !spoken.is_empty(),
        "no cow files found in {}",
        dir.display()
    );

    for _ in 0..6 {
        let got = run(Path::new(OUR_BIN), &["-r", "hello there"], b"", Some(&dir));
        assert!(
            spoken.contains(&got),
            "-r printed a picture that no cow file on {} produces: {}",
            dir.display(),
            String::from_utf8_lossy(&got)
        );
    }
}

/// Wide text is where the documented deviation lives: cowsay counts bytes, so
/// its balloon is too wide for CJK; cowtype counts columns and stays square.
#[test]
fn wide_text_is_measured_in_columns() {
    if !cowsay_works() {
        eprintln!("cowsay not installed, skipping");
        return;
    }
    let ours = run(Path::new(OUR_BIN), &["中文"], b"", cow_dir().as_deref());
    let ours = String::from_utf8_lossy(&ours);
    // Four columns plus one space of padding on each side.
    assert!(
        ours.contains(" ______"),
        "the balloon should be six columns wide: {ours:?}"
    );
    assert!(
        ours.contains("< 中文 >"),
        "the text should be intact: {ours:?}"
    );

    // The same input through cowsay is wider, because it counts six bytes.
    let theirs = run(Path::new("cowsay"), &["中文"], b"", cow_dir().as_deref());
    assert_ne!(
        theirs,
        run(Path::new(OUR_BIN), &["中文"], b"", cow_dir().as_deref()),
        "cowsay should disagree here; if it does not, the deviation note is stale"
    );

    // Wrapping counts columns too: four columns plus a space is five, so the
    // two wide words land on separate balloon lines.
    let wrapped = run(
        Path::new(OUR_BIN),
        &["-W", "5", "中文 中文"],
        b"",
        cow_dir().as_deref(),
    );
    let wrapped = String::from_utf8_lossy(&wrapped);
    assert!(
        wrapped.starts_with(" ______\n/ 中文 \\\n\\ 中文 /\n ------"),
        "two wide words should take two balloon lines: {wrapped:?}"
    );
}
