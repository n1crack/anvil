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

/// Whether stdout gets color.
pub fn enabled() -> bool {
    color(false)
}

/// Rendered `--help` with its plain text (the descriptions) in an off-white, a
/// step softer than the terminal's white; clap's own styles stay as they are.
pub fn help_text(ansi: &str) -> String {
    const SOFT: &str = "\x1b[38;5;252m";
    format!(
        "{SOFT}{}\x1b[0m",
        ansi.replace("\x1b[0m", &format!("\x1b[0m{SOFT}"))
    )
}

/// One Dark's palette: 24-bit where the terminal says it can (COLORTERM), else
/// the nearest of the 256 colors every terminal has.
fn tone(rgb: (u8, u8, u8), ansi256: u8) -> String {
    static TRUE: OnceLock<bool> = OnceLock::new();
    let truecolor = *TRUE
        .get_or_init(|| std::env::var("COLORTERM").is_ok_and(|v| v == "truecolor" || v == "24bit"));
    if truecolor {
        format!("38;2;{};{};{}", rgb.0, rgb.1, rgb.2)
    } else {
        format!("38;5;{ansi256}")
    }
}

/// Names and headings.
pub fn bold(s: impl Display) -> String {
    paint("1", s, false)
}
/// Secondary columns: kinds, paths, hints.
pub fn dim(s: impl Display) -> String {
    paint(&tone((0x7f, 0x84, 0x8e), 245), s, false)
}
/// Something running.
pub fn green(s: impl Display) -> String {
    paint(&tone((0x98, 0xc3, 0x79), 114), s, false)
}
/// Warnings.
pub fn yellow(s: impl Display) -> String {
    paint(&tone((0xe5, 0xc0, 0x7b), 180), s, false)
}
/// Addresses and commands to copy.
pub fn cyan(s: impl Display) -> String {
    paint(&tone((0x61, 0xaf, 0xef), 75), s, false)
}
/// Errors on stdout.
pub fn red(s: impl Display) -> String {
    paint(&tone((0xe0, 0x6c, 0x75), 168), s, false)
}
/// Errors on stderr.
pub fn error(s: impl Display) -> String {
    paint(&tone((0xe0, 0x6c, 0x75), 168), s, true)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_text_softens_plain_text_after_each_reset() {
        let out = help_text("\x1b[1mUsage:\x1b[0m anvil");
        assert_eq!(
            out,
            "\x1b[38;5;252m\x1b[1mUsage:\x1b[0m\x1b[38;5;252m anvil\x1b[0m"
        );
    }
}
