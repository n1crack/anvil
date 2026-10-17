//! Terminal colors for the CLI. Output stays plain when it isn't a terminal or
//! NO_COLOR is set (https://no-color.org); CLICOLOR_FORCE=1 forces color.
//! Pad text before styling it so aligned columns stay aligned.

use std::fmt::Display;
use std::io::IsTerminal;
use std::sync::OnceLock;

fn color(stderr: bool) -> bool {
    static ON: OnceLock<[bool; 2]> = OnceLock::new();
    ON.get_or_init(|| {
        let set = |k| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0");
        let auto = |tty: bool| {
            !set("NO_COLOR")
                && (set("CLICOLOR_FORCE")
                    || tty && std::env::var_os("TERM").is_none_or(|t| t != "dumb"))
        };
        [
            auto(std::io::stdout().is_terminal()),
            auto(std::io::stderr().is_terminal()),
        ]
    })[stderr as usize]
}

fn paint(code: &str, s: impl Display, stderr: bool) -> String {
    if color(stderr) {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// Names and headings.
pub fn bold(s: impl Display) -> String {
    paint("1", s, false)
}
/// Secondary columns: kinds, paths, hints.
pub fn dim(s: impl Display) -> String {
    paint("2", s, false)
}
/// Something running.
pub fn green(s: impl Display) -> String {
    paint("32", s, false)
}
/// Warnings.
pub fn yellow(s: impl Display) -> String {
    paint("33", s, false)
}
/// Addresses and commands to copy.
pub fn cyan(s: impl Display) -> String {
    paint("36", s, false)
}
/// Errors on stdout.
pub fn red(s: impl Display) -> String {
    paint("31", s, false)
}
/// Errors on stderr.
pub fn error(s: impl Display) -> String {
    paint("31", s, true)
}

/// A message with each `command` in it highlighted.
pub fn commands(s: &str) -> String {
    if s.matches('`').count() % 2 == 1 {
        return s.to_string();
    }
    s.split('`')
        .enumerate()
        .map(|(i, part)| match i % 2 {
            1 => cyan(format!("`{part}`")),
            _ => part.to_string(),
        })
        .collect()
}
