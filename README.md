# cowtype

A Rust reimplementation of `cowsay` with a **type mode**: an extra terminal mode
that draws what you type *inside* the cow's speech balloon, so input stands out in
asciinema recordings.

cowtype stays a `cowsay` at heart. Same options, same cow files, same bytes on
stdout for ASCII input. It only adds a mode for the case cowsay cannot serve:
typing live in front of a recorder.

```
$ cowtype "hello world"
 _____________
< hello world >
 -------------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
                ||----w |
                ||     ||
```

## Install

```sh
cargo install --path .
```

cowtype looks for cows where cowsay does: `<prefix>/share/cowsay/cows`,
`<prefix>/share/cowsay/site-cows`, `cowsay/cowpath.d` entries, `$COWPATH`, and the
usual package-manager locations (`/usr/share/cowsay/cows`,
`/usr/local/share/cows`, `/opt/homebrew/share/cowsay/cows`).

Because cowsay picks "say" or "think" from its own name, cowtype does too. Symlink
it and the core binary is a `cowsay` again:

```sh
ln -s "$(command -v cowtype)" /usr/local/bin/cowsay
ln -s "$(command -v cowtype)" /usr/local/bin/cowthink
```

## Type mode

With no message and a terminal on stdin, cowtype becomes a small editor. Local
echo is turned off, so the only place your text appears is the balloon:

```sh
cowtype          # start typing
```

| Key | Action |
| --- | --- |
| `Enter` | start a new balloon line |
| `Ctrl-D` | finish and leave the final picture on screen |
| `Ctrl-C` | abort (exit status 130) |
| `Backspace` | delete a character |
| `Ctrl-U` | clear the line |
| `Ctrl-W` | delete the last word |
| `Ctrl-L` | redraw |
| arrow keys | ignored |

Each keystroke redraws the frame in place, so a recording shows the balloon
growing one character at a time instead of a wall of typed text. Type mode keeps
your line breaks: code is never re-flowed into a paragraph, and an empty line is a
line waiting to be filled. The balloon defaults to the terminal width (minus four
columns) here, so long lines stay readable; `-W` overrides that.

For piped input, `--incremental` brings the same progressive rendering without a
terminal:

```sh
cowsay -f tux --incremental --delay 40 < notes.txt
```

### Recording

```sh
asciinema rec demo.cast -c "cowtype"
```

## Options

The short options match cowsay 3.8.4, including the `-L` and `-N` no-ops and the
`Unknown option:` warning for anything else:

| Option | Meaning |
| --- | --- |
| `-b -d -g -p -s -t -w -y` | borg, dead, greedy, paranoid, stoned, tired, wired, young |
| `-e <eyes>` | custom eyes (first two characters) |
| `-T <tongue>` | custom tongue (first two characters) |
| `-f <cow>` | cow name, or path to a cow file |
| `-r` | random cow; `-C` also allows truecolor cows |
| `-W <columns>` | wrap column (default 40) |
| `-n` | keep whitespace, no re-flow (stdin only) |
| `-l` | list cows: a wrapped column on a terminal, one per line otherwise |
| `-h`, `--help` | help |

Additions:

| Option | Meaning |
| --- | --- |
| `--no-type` | never enter type mode; behave exactly like cowsay |
| `--incremental` | one frame per input line, for piped input |
| `--append` | add frames instead of redrawing in place |
| `--delay <ms>` | pause between incremental frames |
| `--version` | print the version |

As in cowsay, option parsing stops at the first non-option argument, so
`cowtype hi -b` speaks "hi -b".

## Cow files

Any cowsay 3.x cow file works. A cow is a Perl snippet, and cowsay loads it with
`do`; cowtype reads the subset that cow files actually use:

```perl
$the_cow = <<EOC;              # heredoc, interpolating
$the_cow = <<'EOC';            # heredoc, literal backslashes
$the_cow = <<"A" . <<'B';      # concatenated heredocs
$eyes = 'xx';                  # override the face
$eyes = 'xx' unless ($eyes);   # trailing if / unless guard
$eyes .= ($extra x 2);         # append, and repeat a string
$other_eye = chop($eyes);      # chop/chomp the face variables
$balloon_lines[0] =~ tr/_/-/;  # transliterate a balloon line
unshift @balloon_lines, "\n";  # add a balloon line
push @balloon_lines, "x";
```

Templates interpolate `$eyes`, `$tongue`, `$thoughts`, and any variable the file
defines, plus the usual `\\ \$ \n \e` escapes. Anything outside this subset is
reported on stderr and skipped, so a cow that relies on real Perl degrades to its
artwork instead of failing.

## Compatibility

For ASCII input the output is byte-identical to cowsay 3.8.4; `cargo test` proves
it against the installed `cowsay` for every cow file it can find.

Two deliberate differences:

- **Widths count terminal columns.** cowsay counts bytes, which leaves the balloon
  crooked around `café` or `中文`. cowtype measures display width, so the border
  lines up. Pure-ASCII text is unaffected.
- **`-W` is clamped to 2.** cowsay turns `-W 0` or `-W 1` into a degenerate wrap
  and prints nonsense; cowtype keeps a sane minimum.

Everything else, including the odd corners — trailing tabs from Perl's unexpand, a
whitespace-only message producing an empty balloon, cow files that patch their own
balloon — is reproduced on purpose.

## Development

```sh
cargo test          # unit tests, plus parity tests when cowsay is installed
cargo clippy --all-targets
```

Layout:

| File | Contents |
| --- | --- |
| `src/text.rs` | `Text::Wrap` / `Text::Tabs` port: wrapping, tabs, widths |
| `src/balloon.rs` | balloon borders, padding, face flags |
| `src/cow.rs` | cowpath, listing, cow file parsing |
| `src/cli.rs` | cowsay-compatible option parsing |
| `src/type_mode.rs` | type mode: raw terminal, frame redraw, incremental stream |
| `src/main.rs` | wiring |

## License

MIT
