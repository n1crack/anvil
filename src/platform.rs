//! What differs between Unix and Windows: processes and their groups, file modes,
//! the control socket's transport, local time. The rest of Anvil calls these rather
//! than `nix` or `std::os::{unix, windows}`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
pub use uds_windows::{UnixListener, UnixStream};

/// Start a command as the head of its own group, so stopping it stops what it started
/// (`npm run dev` and its node). On Windows: a new process group and no console window,
/// which would otherwise flash up and freeze its output when clicked.
pub trait OwnGroup {
    fn own_group(&mut self) -> &mut Self;
}

impl OwnGroup for Command {
    #[cfg(unix)]
    fn own_group(&mut self) -> &mut Self {
        use std::os::unix::process::CommandExt;
        self.process_group(0)
    }
    #[cfg(windows)]
    fn own_group(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW)
    }
}

/// A pseudo-terminal for a one-off run's output (Unix), so pnpm and the like redraw their
/// progress as in a terminal instead of appending a line per update. Only stdout and
/// stderr are the terminal: stdin stays null and the child gets a session of its own
/// without a controlling terminal, so what asks a question gets no answer and goes on or
/// fails (npx proceeds, `pnpm approve` ends with "Device not configured") instead of hanging.
#[cfg(unix)]
pub struct Pty {
    master: std::os::fd::OwnedFd,
    slave: std::os::fd::OwnedFd,
}

#[cfg(windows)]
pub enum Pty {}

impl Pty {
    /// With `tty` (Unix only): a terminal `columns` × `lines` as `c`'s stdout and stderr,
    /// in a new session, which also makes it a group leader as `own_group` would. `None`
    /// leaves `c` as it is.
    #[cfg(unix)]
    pub fn attach(c: &mut Command, tty: bool, columns: u16, lines: u16) -> io::Result<Option<Pty>> {
        use nix::fcntl::{FcntlArg, FdFlag, fcntl};
        use nix::sys::termios::{OutputFlags, SetArg, tcgetattr, tcsetattr};
        use std::os::unix::process::CommandExt;
        if !tty {
            return Ok(None);
        }
        let size = nix::pty::Winsize {
            ws_row: lines,
            ws_col: columns,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let nix::pty::OpenptyResult { master, slave } = nix::pty::openpty(&size, None)?;
        // Neither end leaks into other children (openpty sets no close-on-exec); the
        // child's stdout and stderr are copies, made by dup2, which keep it open.
        for fd in [&master, &slave] {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
        }
        // \n stays \n in the log, not \r\n.
        let mut t = tcgetattr(&slave)?;
        t.output_flags.remove(OutputFlags::ONLCR);
        tcsetattr(&slave, SetArg::TCSANOW, &t)?;
        c.stdout(slave.try_clone()?).stderr(slave.try_clone()?);
        // SAFETY: setsid is async-signal-safe. A new session has pgid == pid, as
        // process_group(0) would give; no TIOCSCTTY, so the terminal never controls it.
        unsafe { c.pre_exec(|| nix::unistd::setsid().map(drop).map_err(io::Error::from)) };
        Ok(Some(Pty { master, slave }))
    }

    #[cfg(windows)]
    pub fn attach(_: &mut Command, _: bool, _: u16, _: u16) -> io::Result<Option<Pty>> {
        Ok(None)
    }

    /// Once `pid` has been spawned (and the `Command` dropped): copy what it writes into
    /// `log` on a thread of its own, until the terminal closes or shortly after `pid` is
    /// gone (reaped), as background processes it left can keep the terminal open.
    #[cfg(unix)]
    pub fn copy_to(self, mut log: fs::File, pid: u32) {
        use io::{Read, Write};
        use nix::poll::{PollFd, PollFlags, poll};
        use std::os::fd::AsFd;
        use std::time::{Duration, Instant};
        drop(self.slave); // only the child's copies are left
        let master = fs::File::from(self.master);
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let mut gone: Option<Instant> = None;
            // ponytail: `alive` asks by pid; a pid reused right after the reap keeps this
            // reading until EOF or that process ends, which is harmless.
            while gone.is_none_or(|t| t.elapsed() < Duration::from_millis(300)) {
                let mut fds = [PollFd::new(master.as_fd(), PollFlags::POLLIN)];
                match poll(&mut fds, 100u16) {
                    Ok(0) | Err(nix::errno::Errno::EINTR) => {}
                    Err(_) => break,
                    Ok(_) => match (&master).read(&mut buf) {
                        Ok(n @ 1..) => {
                            if log.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                        // EOF; on Linux EIO once the last copy of the slave is closed.
                        _ => break,
                    },
                }
                if gone.is_none() && !alive(pid) {
                    gone = Some(Instant::now());
                }
            }
        });
    }

    #[cfg(windows)]
    pub fn copy_to(self, _: fs::File, _: u32) {
        match self {}
    }
}

/// `Command::new` for every program Anvil runs. On Windows a program started from the
/// app, which has no console, would open a console window of its own; from a terminal
/// it shares the terminal's.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg_attr(unix, allow(unused_mut))]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        // SAFETY: no arguments; returns this process's console window or null.
        let console = unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() };
        if console.is_null() {
            no_window(&mut c);
        }
    }
    c
}

/// A shell running `script`: `/bin/sh -c`, or on Windows `cmd /C`. On Unix `tag` becomes
/// the shell's `$0`, which `ps` shows (a crashed Anvil's leftovers are found by it).
pub fn shell(script_in: &str, tag: Option<&str>) -> Command {
    #[cfg(unix)]
    {
        let mut c = command("/bin/sh");
        c.args(["-c", script_in]).args(tag);
        c
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = tag;
        // Commands name variables the sh way (`--port $PORT`); cmd reads `%PORT%`.
        let mut script = String::with_capacity(script_in.len());
        let mut rest = script_in;
        while let Some(i) = rest.find('$') {
            script.push_str(&rest[..i]);
            let name: String = rest[i + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                script.push('$');
            } else {
                script.push_str(&format!("%{name}%"));
            }
            rest = &rest[i + 1 + name.len()..];
        }
        script.push_str(rest);
        let mut c = command("cmd.exe");
        // As typed: Rust's quoting (`\"`) means nothing to cmd; /S strips the outer quotes.
        c.raw_arg(format!("/D /S /C \"{script}\""));
        c
    }
}

/// One word for a `shell` command line, whatever it holds: sh's `'…'`, or on Windows
/// `"…"` (cmd has no single quotes), left bare when it needs no quotes.
pub fn shell_quote(word: &str) -> String {
    if cfg!(windows) {
        let plain = !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/\\@+=,".contains(c));
        if plain {
            word.to_string()
        } else {
            format!("\"{}\"", word.replace('"', "\"\""))
        }
    } else {
        crate::shims::sh_quote(word)
    }
}

/// `shell` as a program and its arguments.
pub fn shell_args(script: &str) -> (PathBuf, Vec<String>) {
    if cfg!(windows) {
        (
            "cmd.exe".into(),
            vec!["/D".into(), "/C".into(), script.into()],
        )
    } else {
        ("/bin/sh".into(), vec!["-c".into(), script.into()])
    }
}

/// Open a file, folder or web address as the desktop does: macOS `open`, Linux
/// `xdg-open`; on Windows the program registered for it.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        // Exits 0 (explorer.exe exits 1 on success) and takes any URL as one argument.
        let mut c = command("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(target);
        c
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = command("open");
        c.arg(target);
        c
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut c = command("xdg-open");
        c.arg(target);
        c
    }
}

/// Open `target` with the application `app`: macOS `open -a`; on Windows and Linux `app`
/// is the program to run (`code`, a full path).
pub fn open_with(app: &str, target: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        let mut c = command("cmd.exe");
        // `start` finds programs the way the Run dialog does (App Paths), and returns.
        if let Some(program) = desktop_program(app) {
            let mut args: Vec<String> = match app {
                "Sourcetree" => vec!["-f".into()],
                "GitKraken" => vec!["--path".into()],
                _ => vec![],
            };
            args.push(target.as_ref().to_string_lossy().into_owned());
            return start_program(&program, &args, None);
        }
        c.args(["/D", "/C", "start", "\"\"", app]).arg(target);
        c
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = command("open");
        c.args(["-a", app]).arg(target);
        c
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut c = command(desktop_program(app).unwrap_or_else(|| app.into()));
        c.arg(target);
        c
    }
}

/// Show a file in the file manager: selected in Finder; on Windows and Linux its folder.
pub fn reveal(file: &Path) -> Command {
    if !cfg!(target_os = "macos") {
        open(file.parent().unwrap_or(file))
    } else {
        let mut c = command("open");
        c.arg("-R").arg(file);
        c
    }
}

/// A terminal window in `dir`: `app` when set (macOS `open -a`, a program on Windows and
/// Linux), else Terminal, or on Windows Windows Terminal, else PowerShell in a console,
/// or on Linux `$TERMINAL`, else the first terminal found (`linux_terminal`).
pub fn terminal(app: Option<&str>, dir: &Path) -> Command {
    #[cfg(windows)]
    {
        if let Some(app) = app {
            if let Some(program) = desktop_program(app) {
                let args = if app == "Windows Terminal" {
                    vec!["-d".to_string(), dir.display().to_string()]
                } else {
                    vec![]
                };
                return start_program(&program, &args, Some(dir));
            }
            let mut c = command(app);
            c.current_dir(dir);
            return c;
        }
        let wt = std::env::var_os("LOCALAPPDATA")
            .map(|d| Path::new(&d).join(r"Microsoft\WindowsApps\wt.exe"))
            .filter(|p| p.exists());
        if let Some(wt) = wt {
            let mut c = command(wt);
            c.arg("-d").arg(dir);
            return c;
        }
        let mut c = command("cmd.exe");
        c.args(["/D", "/C", "start", "\"\"", "powershell.exe", "-NoExit"])
            .current_dir(dir);
        c
    }
    #[cfg(target_os = "macos")]
    {
        open_with(app.unwrap_or("Terminal"), dir)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let program = match app {
            Some(app) => desktop_program(app).unwrap_or_else(|| app.into()),
            None => linux_terminal().into(),
        };
        let mut c = command(program);
        // Terminals open where they are started.
        c.current_dir(dir);
        c
    }
}

/// The terminal Linux opens: `$TERMINAL`, else the first of the distributions' and
/// desktops' own that is installed, else xterm.
#[cfg(all(unix, not(target_os = "macos")))]
fn linux_terminal() -> String {
    if let Some(t) = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty()) {
        return t;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    [
        "xdg-terminal-exec",
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "ptyxis",
        "kgx",
        "alacritty",
        "kitty",
        "foot",
        "wezterm",
    ]
    .into_iter()
    .find(|t| std::env::split_paths(&path).any(|d| d.join(t).is_file()))
    .unwrap_or("xterm")
    .to_string()
}

/// Linux: the apps Settings offers, by name, each with the programs that are it: its
/// own command, then its Flatpak id. The first one found is the one that runs.
#[cfg(all(unix, not(target_os = "macos")))]
pub const DESKTOP_APPS: [(&str, &str, &[&str]); 31] = [
    ("terminal", "Ghostty", &["ghostty", "com.mitchellh.ghostty"]),
    (
        "terminal",
        "Alacritty",
        &["alacritty", "org.alacritty.Alacritty"],
    ),
    ("terminal", "kitty", &["kitty"]),
    (
        "terminal",
        "WezTerm",
        &["wezterm", "org.wezfurlong.wezterm"],
    ),
    ("terminal", "foot", &["foot"]),
    ("terminal", "GNOME Terminal", &["gnome-terminal"]),
    ("terminal", "Ptyxis", &["ptyxis", "app.devsuite.Ptyxis"]),
    ("terminal", "Console", &["kgx", "org.gnome.Console"]),
    ("terminal", "Konsole", &["konsole", "org.kde.konsole"]),
    ("terminal", "Warp", &["warp-terminal"]),
    ("terminal", "Tilix", &["tilix", "com.gexperts.Tilix"]),
    (
        "editor",
        "Visual Studio Code",
        &["code", "com.visualstudio.code"],
    ),
    ("editor", "VSCodium", &["codium", "com.vscodium.codium"]),
    ("editor", "Cursor", &["cursor"]),
    ("editor", "Windsurf", &["windsurf"]),
    // Arch packages Zed as `zeditor`: `zed` is also ZFS's event daemon.
    ("editor", "Zed", &["zeditor", "zed", "dev.zed.Zed"]),
    ("editor", "Sublime Text", &["subl", "com.sublimetext.three"]),
    (
        "editor",
        "PhpStorm",
        &["phpstorm", "com.jetbrains.PhpStorm"],
    ),
    (
        "editor",
        "WebStorm",
        &["webstorm", "com.jetbrains.WebStorm"],
    ),
    (
        "editor",
        "IntelliJ IDEA",
        &[
            "idea",
            "intellij-idea-ultimate",
            "com.jetbrains.IntelliJ-IDEA-Ultimate",
            "com.jetbrains.IntelliJ-IDEA-Community",
        ],
    ),
    ("editor", "Kate", &["kate", "org.kde.kate"]),
    (
        "editor",
        "Text Editor",
        &["gnome-text-editor", "org.gnome.TextEditor"],
    ),
    ("git", "GitKraken", &["gitkraken", "com.axosoft.GitKraken"]),
    ("git", "Sublime Merge", &["smerge", "com.sublimemerge.App"]),
    ("git", "SmartGit", &["smartgit", "com.syntevo.SmartGit"]),
    (
        "git",
        "GitHub Desktop",
        &["github-desktop", "io.github.shiftey.Desktop"],
    ),
    ("git", "Gitg", &["gitg", "org.gnome.gitg"]),
    ("git", "GitButler", &["gitbutler-tauri", "gitbutler"]),
    ("git", "Gittyup", &["gittyup", "com.github.Murmele.Gittyup"]),
    ("git", "GitAhead", &["gitahead"]),
    ("git", "QGit", &["qgit"]),
];

/// Linux: the `kind` apps of `DESKTOP_APPS` (terminal, editor, git) installed here.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn desktop_apps(kind: &str) -> Vec<String> {
    DESKTOP_APPS
        .iter()
        .filter(|(k, name, _)| *k == kind && desktop_program(name).is_some())
        .map(|(_, name, _)| name.to_string())
        .collect()
}

/// Linux: the program an app from Settings runs: one of `DESKTOP_APPS`' by its name,
/// else a program picked by path or found by name.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn desktop_program(app: &str) -> Option<PathBuf> {
    if app.contains('/') {
        return Path::new(app).is_file().then(|| app.into());
    }
    let programs: &[&str] = DESKTOP_APPS
        .iter()
        .find(|(_, name, _)| *name == app)
        .map_or(&[], |(_, _, p)| p);
    let home = crate::user_home();
    // Beside PATH: where Flatpak exports its apps and JetBrains Toolbox its launchers,
    // which a desktop session's PATH often leaves out.
    let dirs: Vec<PathBuf> = std::env::split_paths(&crate::env_path())
        .chain([
            home.join(".local/bin"),
            home.join(".local/share/flatpak/exports/bin"),
            PathBuf::from("/var/lib/flatpak/exports/bin"),
            home.join(".local/share/JetBrains/Toolbox/scripts"),
        ])
        .collect();
    programs
        .iter()
        .copied()
        .chain(programs.is_empty().then_some(app))
        .find_map(|p| dirs.iter().map(|d| d.join(p)).find(|f| f.is_file()))
}

/// Windows: the apps Settings offers, and where each installs, relative to
/// `%LOCALAPPDATA%`, `%ProgramFiles%` or `%ProgramFiles(x86)%`. `*` is a version in a
/// folder's name, the newest first.
pub const WINDOWS_APPS: [(&str, &str, &[&str]); 21] = [
    (
        "terminal",
        "Windows Terminal",
        &[r"Microsoft\WindowsApps\wt.exe"],
    ),
    ("terminal", "PowerShell 7", &[r"PowerShell\7\pwsh.exe"]),
    ("terminal", "Git Bash", &[r"Git\git-bash.exe"]),
    ("terminal", "WezTerm", &[r"WezTerm\wezterm-gui.exe"]),
    ("terminal", "Alacritty", &[r"Alacritty\alacritty.exe"]),
    (
        "editor",
        "Visual Studio Code",
        &[
            r"Programs\Microsoft VS Code\Code.exe",
            r"Microsoft VS Code\Code.exe",
        ],
    ),
    ("editor", "Cursor", &[r"Programs\cursor\Cursor.exe"]),
    ("editor", "Zed", &[r"Programs\Zed\Zed.exe", r"Zed\Zed.exe"]),
    ("editor", "Windsurf", &[r"Programs\Windsurf\Windsurf.exe"]),
    (
        "editor",
        "Sublime Text",
        &[r"Sublime Text\sublime_text.exe"],
    ),
    ("editor", "Notepad++", &[r"Notepad++\notepad++.exe"]),
    (
        "editor",
        "PhpStorm",
        &[
            r"Programs\PhpStorm\bin\phpstorm64.exe",
            r"JetBrains\PhpStorm*\bin\phpstorm64.exe",
        ],
    ),
    (
        "editor",
        "WebStorm",
        &[
            r"Programs\WebStorm\bin\webstorm64.exe",
            r"JetBrains\WebStorm*\bin\webstorm64.exe",
        ],
    ),
    (
        "editor",
        "IntelliJ IDEA",
        &[
            r"Programs\IntelliJ IDEA*\bin\idea64.exe",
            r"JetBrains\IntelliJ IDEA*\bin\idea64.exe",
        ],
    ),
    (
        "editor",
        "Rider",
        &[
            r"Programs\Rider\bin\rider64.exe",
            r"JetBrains\Rider*\bin\rider64.exe",
            r"JetBrains\JetBrains Rider*\bin\rider64.exe",
        ],
    ),
    (
        "git",
        "GitHub Desktop",
        &[r"GitHubDesktop\GitHubDesktop.exe"],
    ),
    ("git", "Fork", &[r"Fork\Fork.exe"]),
    (
        "git",
        "Sourcetree",
        &[
            r"Atlassian\SourceTree\SourceTree.exe",
            r"SourceTree\SourceTree.exe",
        ],
    ),
    ("git", "GitKraken", &[r"gitkraken\gitkraken.exe"]),
    (
        "git",
        "Sublime Merge",
        &[r"Sublime Merge\sublime_merge.exe"],
    ),
    ("git", "SmartGit", &[r"SmartGit\bin\smartgit.exe"]),
];

/// The file `pattern` (see `WINDOWS_APPS`) finds under `root`. By the link itself, as
/// the programs in WindowsApps are links that can't be followed.
#[cfg_attr(not(windows), allow(dead_code))]
fn program_under(root: &Path, pattern: &str) -> Option<PathBuf> {
    let mut found = vec![root.to_path_buf()];
    for part in pattern.split('\\') {
        found = found
            .iter()
            .flat_map(|dir| match part.split_once('*') {
                None => vec![dir.join(part)],
                Some((start, end)) => {
                    let (start, end) = (start.to_lowercase(), end.to_lowercase());
                    let mut versions: Vec<PathBuf> = fs::read_dir(dir)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .filter(|e| {
                            let name = e.file_name().to_string_lossy().to_lowercase();
                            name.starts_with(&start) && name.ends_with(&end)
                        })
                        .map(|e| e.path())
                        .collect();
                    versions.sort();
                    versions.reverse();
                    versions
                }
            })
            .collect();
    }
    found
        .into_iter()
        .find(|p| fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir()))
}

/// Windows: the `kind` apps of `WINDOWS_APPS` (terminal, editor, git) installed here.
#[cfg(windows)]
pub fn desktop_apps(kind: &str) -> Vec<String> {
    WINDOWS_APPS
        .iter()
        .filter(|(k, name, _)| *k == kind && desktop_program(name).is_some())
        .map(|(_, name, _)| name.to_string())
        .collect()
}

/// Windows: the program an app from Settings runs: one of `WINDOWS_APPS`' by its name,
/// else a program picked by path.
#[cfg(windows)]
pub fn desktop_program(app: &str) -> Option<PathBuf> {
    let path = Path::new(app);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let (_, _, patterns) = WINDOWS_APPS.iter().find(|(_, name, _)| *name == app)?;
    ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(std::env::var_os)
        .find_map(|root| {
            patterns
                .iter()
                .find_map(|pattern| program_under(Path::new(&root), pattern))
        })
}

/// `start`, which returns at once and gives a console program a window of its own.
/// Written out, as `start` takes its first quoted word for a title.
#[cfg(windows)]
fn start_program(program: &Path, args: &[String], dir: Option<&Path>) -> Command {
    use std::os::windows::process::CommandExt;
    let mut line = String::from("/D /C start \"\"");
    if let Some(dir) = dir {
        line.push_str(&format!(" /D \"{}\"", dir.display()));
    }
    line.push_str(&format!(" \"{}\"", program.display()));
    for arg in args {
        line.push_str(&format!(" \"{arg}\""));
    }
    let mut c = command("cmd.exe");
    c.raw_arg(line);
    c
}

/// The process listening on `port` in `netstat -ano` output: by its addresses, as
/// Windows translates the state column.
#[cfg_attr(not(windows), allow(dead_code))]
fn netstat_listener(out: &str, port: u16) -> Option<u32> {
    let local_port = format!(":{port}");
    out.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // The state can be more than one word once translated.
        let [proto, local, remote, .., pid] = fields[..] else {
            return None;
        };
        (proto.eq_ignore_ascii_case("TCP")
            && local.ends_with(&local_port)
            && remote.ends_with(":0"))
        .then(|| pid.parse().ok())
        .flatten()
    })
}

/// The program in a line of `tasklist /FO CSV /NH`, said so that it means something
/// when it is only WSL's relay for a program inside WSL.
#[cfg_attr(not(windows), allow(dead_code))]
fn tasklist_program(out: &str) -> Option<String> {
    let name = out.trim_start().strip_prefix('"')?.split('"').next()?;
    Some(if name.eq_ignore_ascii_case("wslrelay.exe") {
        "a program inside WSL (wslrelay.exe)".to_string()
    } else {
        name.to_string()
    })
}

/// What listens on `port`, when the system can say (Windows).
pub fn port_owner(port: u16) -> Option<String> {
    #[cfg(windows)]
    {
        let text = |c: &mut Command| {
            let out = no_window(c).output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let pid = netstat_listener(&text(command("netstat").arg("-ano"))?, port)?;
        let list =
            text(command("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]))?;
        tasklist_program(&list)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        None
    }
}

/// No console window for a background child (Windows); nothing elsewhere.
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Whether `pid` names a running process.
#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    use nix::sys::signal::kill;
    kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain Win32 calls on a handle opened and closed here.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE as u32;
        CloseHandle(h);
        ok
    }
}

/// Whether the group `pid` leads still has a member (Unix), or `pid` itself runs (Windows,
/// which has no groups to ask).
#[cfg(unix)]
pub fn group_alive(pid: u32) -> bool {
    nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(windows)]
pub fn group_alive(pid: u32) -> bool {
    alive(pid)
}

/// Ask `pid` (with `group`, its whole group) to shut down: SIGTERM. Windows has no such
/// request for a windowless process, so there it is `kill`. False when nothing got it.
#[cfg(unix)]
pub fn term(pid: u32, group: bool) -> bool {
    signal(pid, group, nix::sys::signal::Signal::SIGTERM)
}

#[cfg(windows)]
pub fn term(pid: u32, group: bool) -> bool {
    alive(pid) && {
        kill(pid, group);
        true
    }
}

/// Force `pid` (with `group`, its group; on Windows its process tree) to end.
#[cfg(unix)]
pub fn kill(pid: u32, group: bool) {
    signal(pid, group, nix::sys::signal::Signal::SIGKILL);
}

#[cfg(windows)]
pub fn kill(pid: u32, group: bool) {
    if group {
        let mut cmd = command("taskkill");
        cmd.args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let _ = no_window(&mut cmd).status();
        return;
    }
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
    };
    // SAFETY: plain Win32 calls on a handle opened and closed here.
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            // Termination is asynchronous: wait (briefly) until its ports are free.
            WaitForSingleObject(h, 3000);
            CloseHandle(h);
        }
    }
}

#[cfg(unix)]
fn signal(pid: u32, group: bool, s: nix::sys::signal::Signal) -> bool {
    use nix::sys::signal::{kill, killpg};
    let p = nix::unistd::Pid::from_raw(pid as i32);
    (if group { killpg(p, s) } else { kill(p, s) }).is_ok()
}

/// When `pid` started, in a form that tells it from a later process given the same pid.
#[cfg(unix)]
pub fn started(pid: u32) -> Option<String> {
    // As `ps` prints it (`Sat Oct 18 10:02:03 2026`), in one locale and time zone: `ps`
    // follows LANG and TZ, and an Anvil started from a terminal has others than one
    // started from Finder.
    let o = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(windows)]
pub fn started(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: plain Win32 calls on a handle opened and closed here.
    let ok = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let ok = GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(h);
        ok
    };
    // 100 ns ticks since 1601: unique enough to tell two processes apart.
    ok.then(|| ((created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64).to_string())
}

/// The command line of `pid`, where it can be read.
#[cfg(unix)]
pub fn command_line(pid: u32) -> Option<String> {
    let o = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Every process as `(pid, ppid, pgid, command line)`, from one `ps`.
#[cfg(unix)]
pub fn processes() -> Vec<(u32, u32, u32, String)> {
    let Ok(o) = Command::new("ps")
        .args(["-axo", "pid=,ppid=,pgid=,command="])
        .env("LC_ALL", "C")
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| {
            // Three numbers, then the command line as it is (a home may hold spaces).
            let field = |s: &str| {
                s.trim_start()
                    .split_once(' ')
                    .map(|(a, b)| (a.to_string(), b.to_string()))
            };
            let (pid, rest) = field(l)?;
            let (ppid, rest) = field(&rest)?;
            let (pgid, command) = field(&rest)?;
            Some((
                pid.parse().ok()?,
                ppid.parse().ok()?,
                pgid.parse().ok()?,
                command.trim().to_string(),
            ))
        })
        .collect()
}

/// Process-group leaders whose parent is gone (launchd adopted them), with their
/// command lines: what a killed Anvil's dev shells become.
// ponytail: "parent gone" is ppid 1; Linux with a subreaper (systemd --user) reparents
// elsewhere and these are missed there, pid files still cover them.
#[cfg(unix)]
pub fn orphan_leaders() -> Vec<(u32, String)> {
    processes()
        .into_iter()
        .filter(|(pid, ppid, pgid, _)| *ppid == 1 && pgid == pid)
        .map(|(pid, _, _, c)| (pid, c))
        .collect()
}

/// The descendants of the process groups `groups` that left them (a `setsid`, as Next's
/// telemetry flusher does), with their command lines, from a `processes` table. Killing
/// a group misses them, and once their group has ended they belong to launchd and
/// nothing says whose they were: read the table before stopping the group.
pub fn left_groups(table: &[(u32, u32, u32, String)], groups: &[u32]) -> Vec<(u32, String)> {
    let mut seen: std::collections::BTreeSet<u32> = table
        .iter()
        .filter(|p| groups.contains(&p.2))
        .map(|p| p.0)
        .collect();
    let mut todo: Vec<u32> = seen.iter().copied().collect();
    let mut out = vec![];
    while let Some(parent) = todo.pop() {
        for (pid, _, pgid, c) in table.iter().filter(|p| p.1 == parent) {
            if seen.insert(*pid) {
                todo.push(*pid);
                if !groups.contains(pgid) {
                    out.push((*pid, c.clone()));
                }
            }
        }
    }
    out
}

/// Windows: no tag on the command line to recognise (`shell`); pid files only.
#[cfg(windows)]
pub fn orphan_leaders() -> Vec<(u32, String)> {
    vec![]
}

/// Windows: `kill` ends a whole tree (`taskkill /T`) and there are no groups to leave.
#[cfg(windows)]
pub fn processes() -> Vec<(u32, u32, u32, String)> {
    vec![]
}

/// Windows keeps the command line in the other process's memory; its program's path
/// says as much here (Caddy and PHP live in Anvil's home).
#[cfg(windows)]
pub fn command_line(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: plain Win32 calls on a handle opened and closed here; `buf` outlives them.
    let ok = unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        ok
    };
    ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

/// `fs::canonicalize` without Windows' `\\\\?\\` prefix on an ordinary drive path: that
/// form breaks comparisons with paths written the usual way and reads oddly to people.
pub fn canonicalize(p: impl AsRef<Path>) -> io::Result<PathBuf> {
    let real = fs::canonicalize(p)?;
    #[cfg(windows)]
    {
        let s = real.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\")
            && rest.as_bytes().get(1) == Some(&b':')
            && !rest.starts_with("UNC")
            && rest.len() < 260
        {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(real)
}

/// Set a file's Unix mode; Windows has no modes (a profile folder is private already).
pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// Whether a file can be run: any x bit on Unix, an .exe/.cmd/.bat/.com on Windows.
pub fn is_executable(path: &Path, m: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = path;
        m.is_file() && m.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        m.is_file()
            && path.extension().is_some_and(|e| {
                ["exe", "cmd", "bat", "com"]
                    .iter()
                    .any(|x| e.eq_ignore_ascii_case(x))
            })
    }
}

/// `name` as a program file: `name.exe` on Windows.
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The separator of PATH-like lists.
pub const PATH_SEP: char = if cfg!(windows) { ';' } else { ':' };

/// Replace this process with `cmd` (Unix `exec`). Windows can't, so it runs `cmd` with
/// this console and exits with its code.
pub fn exec(cmd: &mut Command) -> io::Error {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    }
    #[cfg(windows)]
    {
        // Ctrl+C reaches both; the child decides, and this one waits for it.
        let _ = ctrlc::set_handler(|| {});
        match cmd.status() {
            Ok(st) => std::process::exit(st.code().unwrap_or(1)),
            Err(e) => e,
        }
    }
}

/// Make `link` run `target` (a file next to it): a symlink on Unix, a copy on Windows,
/// where symlinks need Developer Mode.
pub fn link_program(target: &str, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        let from = link.parent().unwrap_or(Path::new(".")).join(target);
        fs::copy(from, link).map(|_| ())
    }
}

/// The first argument that makes an Anvil binary run a program without administrator
/// rights instead of doing its usual work (`restricted_entry`).
#[cfg(windows)]
const RESTRICTED_ARG: &str = "--anvil-without-admin";

/// `command`, except that a program started by an elevated Anvil on Windows loses its
/// administrator rights (through `restricted_entry` in this same binary).
pub fn command_without_admin(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    if elevated()
        && let Ok(me) = std::env::current_exe()
    {
        let mut c = command(me);
        c.arg(RESTRICTED_ARG).arg(program.as_ref());
        return c;
    }
    command(program)
}

/// First thing in `main`: when this process was started by `command_without_admin`, run
/// the program it names and end with its exit code. Elsewhere nothing happens.
pub fn restricted_entry() {
    #[cfg(windows)]
    {
        let mut args = std::env::args_os().skip(1);
        if args.next().is_some_and(|a| a == RESTRICTED_ARG) {
            let argv: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
            let code = run_without_admin(&argv).unwrap_or_else(|e| {
                eprintln!(
                    "Couldn't start {} without administrator rights: {e}",
                    argv[0]
                );
                1
            });
            std::process::exit(code as i32);
        }
    }
}

/// `arg` as a Windows command line needs it, read back by the program as one argument.
#[cfg(any(windows, test))]
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => {
                slashes += 1;
                continue;
            }
            // Backslashes before a quote are doubled, and the quote escaped.
            '"' => out.push_str(&"\\".repeat(slashes * 2 + 1)),
            _ => out.push_str(&"\\".repeat(slashes)),
        }
        slashes = 0;
        out.push(c);
    }
    // Before the closing quote they would escape it.
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

/// Run `argv` with the Administrators and Power Users groups turned off in its token, as
/// PostgreSQL's own `pg_ctl` starts the server, with this process's output; its exit code.
/// It's in a job with this process, so it can't outlive it (an Anvil that crashed
/// and whose leftovers the next one ends by this process's pid).
#[cfg(windows)]
fn run_without_admin(argv: &[String]) -> std::io::Result<u32> {
    use std::io::Error;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_ALL};
    use windows_sys::Win32::Security::{
        ACL, ACL_REVISION, AddAccessAllowedAce, CreateRestrictedToken, CreateWellKnownSid,
        GetTokenInformation, InitializeAcl, SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES,
        SetTokenInformation, TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL, TOKEN_USER, TokenDefaultDacl,
        TokenUser, WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessAsUserW, GetCurrentProcess,
        GetExitCodeProcess, INFINITE, OpenProcessToken, PROCESS_INFORMATION, ResumeThread,
        STARTF_USESTDHANDLES, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    };
    let check = |ok: i32| {
        if ok != 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    };
    let line = argv
        .iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let mut line: Vec<u16> = std::ffi::OsStr::new(&line)
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: Win32 calls on buffers that outlive them; the process exits right after,
    // which closes the handles left open.
    unsafe {
        let mut own = null_mut();
        check(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ALL_ACCESS,
            &mut own,
        ))?;
        let mut sids = [[0u8; SECURITY_MAX_SID_SIZE as usize]; 2];
        let mut off: [SID_AND_ATTRIBUTES; 2] = std::mem::zeroed();
        for (i, kind) in [WinBuiltinAdministratorsSid, WinBuiltinPowerUsersSid]
            .into_iter()
            .enumerate()
        {
            let mut size = SECURITY_MAX_SID_SIZE;
            check(CreateWellKnownSid(
                kind,
                null_mut(),
                sids[i].as_mut_ptr().cast(),
                &mut size,
            ))?;
            off[i].Sid = sids[i].as_mut_ptr().cast();
        }
        let mut token = null_mut();
        check(CreateRestrictedToken(
            own,
            0,
            2,
            off.as_ptr(),
            0,
            null(),
            0,
            null(),
            &mut token,
        ))?;
        // An elevated token's default permissions name only Administrators, just turned
        // off: what the program creates would be closed to itself. usize: TOKEN_USER's
        // and ACL's alignment.
        let mut user = [0usize; 64];
        let mut size = 0;
        check(GetTokenInformation(
            token,
            TokenUser,
            user.as_mut_ptr().cast(),
            size_of_val(&user) as u32,
            &mut size,
        ))?;
        let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let mut acl = [0usize; 32];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        check(InitializeAcl(
            acl_ptr,
            size_of_val(&acl) as u32,
            ACL_REVISION,
        ))?;
        check(AddAccessAllowedAce(acl_ptr, ACL_REVISION, GENERIC_ALL, sid))?;
        let dacl = TOKEN_DEFAULT_DACL {
            DefaultDacl: acl_ptr,
        };
        check(SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&raw const dacl).cast(),
            size_of::<TOKEN_DEFAULT_DACL>() as u32,
        ))?;

        let job = CreateJobObjectW(null(), null());
        if job.is_null() {
            return Err(Error::last_os_error());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        check(SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ))?;

        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESTDHANDLES;
        si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        si.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        si.hStdError = GetStdHandle(STD_ERROR_HANDLE);
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        check(CreateProcessAsUserW(
            token,
            null(),
            line.as_mut_ptr(),
            null(),
            null(),
            1,
            // The app has no console to share: without this PostgreSQL would open a window.
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            null(),
            null(),
            &si,
            &mut pi,
        ))?;
        if let Err(e) = check(AssignProcessToJobObject(job, pi.hProcess)) {
            // Suspended and outside the job: nothing else would ever end it.
            TerminateProcess(pi.hProcess, 1);
            return Err(e);
        }
        ResumeThread(pi.hThread);
        CloseHandle(pi.hThread);
        WaitForSingleObject(pi.hProcess, INFINITE);
        let mut code = 1;
        check(GetExitCodeProcess(pi.hProcess, &mut code))?;
        Ok(code)
    }
}

/// Whether a program listens on TCP `port`, at any address, IPv4 or IPv6, from
/// Windows' own table of listeners: no connection is made, so it takes no time. None
/// when the table can't be read.
#[cfg(windows)]
pub fn tcp_listening(port: u16) -> Option<bool> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    // The port sits in a row's low 16 bits, in network byte order.
    let wanted = u32::from(port.to_be());
    let ports = |family: u32| -> Option<Vec<u32>> {
        // The table can grow between asking its size and reading it: a few tries.
        let mut size = 0u32;
        for _ in 0..4 {
            let mut buf = vec![0u32; (size as usize).div_ceil(4).max(1)];
            // SAFETY: `buf` holds `size` bytes, u32-aligned like the table.
            let r = unsafe {
                GetExtendedTcpTable(
                    buf.as_mut_ptr().cast(),
                    &mut size,
                    0,
                    family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if r == 122 {
                // ERROR_INSUFFICIENT_BUFFER: `size` now says how much.
                continue;
            }
            if r != 0 {
                return None;
            }
            let n = buf[0] as usize;
            // Rows follow the count; the port is a row's third u32 (IPv4) or sixth (IPv6).
            let (row, at) = if family == 2 {
                (size_of::<MIB_TCPROW_OWNER_PID>() / 4, 2)
            } else {
                (size_of::<MIB_TCP6ROW_OWNER_PID>() / 4, 5)
            };
            let rows = buf.get(1..1 + n * row)?;
            return Some(rows.chunks_exact(row).map(|r| r[at] & 0xffff).collect());
        }
        None
    };
    // AF_INET, AF_INET6.
    let v4 = ports(2)?;
    let v6 = ports(23)?;
    Some(v4.contains(&wanted) || v6.contains(&wanted))
}

/// Whether this process already has administrator rights (Windows: an elevated token),
/// so a step that needs them can run without asking.
#[cfg(windows)]
pub fn elevated() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: plain Win32 calls on this process's token, opened and closed here.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut e as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0;
        CloseHandle(token);
        ok && e.TokenIsElevated != 0
    }
}

/// Every process: `(pid, parent pid, resident bytes, CPU % over its life, program)`, as
/// `ps -o pid,ppid,rss,%cpu,command` gives them on a Mac. Windows only; the Mac asks `ps`.
#[cfg(windows)]
pub fn process_table() -> Vec<(u32, u32, u64, f32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let ticks = |t: &FILETIME| (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64;
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut out = vec![];
    // SAFETY: plain Win32 calls; the snapshot and each process handle are closed here,
    // and every struct passed is sized and lives on this stack.
    unsafe {
        let mut now = zero;
        GetSystemTimeAsFileTime(&mut now);
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let name_len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
            let name = String::from_utf16_lossy(&e.szExeFile[..name_len]);
            let (mut rss, mut cpu) = (0, 0.0);
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
            if !h.is_null() {
                let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                m.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                if GetProcessMemoryInfo(h, &mut m, m.cb) != 0 {
                    rss = m.WorkingSetSize as u64;
                }
                let (mut c, mut x, mut k, mut u) = (zero, zero, zero, zero);
                if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0 {
                    let alive = ticks(&now).saturating_sub(ticks(&c)).max(1);
                    cpu = (ticks(&k) + ticks(&u)) as f32 / alive as f32 * 100.0;
                }
                CloseHandle(h);
            }
            out.push((e.th32ProcessID, e.th32ParentProcessID, rss, cpu, name));
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

/// Windows: add to this process's PATH what the user's and the machine's PATH (the
/// registry) gained since it started, so a program just installed is found without a
/// restart. Elsewhere nothing changes.
pub fn refresh_path() {
    #[cfg(windows)]
    {
        let read = |key: &str| {
            let out = command("reg")
                .args(["query", key, "/v", "Path"])
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.lines()
                .find_map(|l| l.trim_start().strip_prefix("Path"))
                .and_then(|l| l.split_once("REG_"))
                .and_then(|(_, r)| r.split_once(char::is_whitespace))
                .map(|(_, v)| v.trim().to_string())
        };
        // %SystemRoot% and the like, as REG_EXPAND_SZ holds them.
        let expand = |dir: &str| {
            let mut out = String::new();
            let mut parts = dir.split('%');
            out.push_str(parts.next().unwrap_or_default());
            let mut var = true;
            for p in parts {
                if var {
                    match std::env::var(p) {
                        Ok(v) if !p.is_empty() => out.push_str(&v),
                        _ => {
                            out.push('%');
                            out.push_str(p);
                            out.push('%');
                        }
                    }
                } else {
                    out.push_str(p);
                }
                var = !var;
            }
            out
        };
        let mut path = std::env::var("PATH").unwrap_or_default();
        let have = |path: &str, dir: &str| {
            path.split(';').any(|d| {
                d.trim_end_matches('\\')
                    .eq_ignore_ascii_case(dir.trim_end_matches('\\'))
            })
        };
        for key in [
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            r"HKCU\Environment",
        ] {
            for dir in read(key)
                .unwrap_or_default()
                .split(';')
                .filter(|d| !d.is_empty())
            {
                let dir = expand(dir);
                if !have(&path, &dir) {
                    path = format!("{path};{dir}");
                }
            }
        }
        // SAFETY: the app sets PATH like this at start too, before its threads read it;
        // here a setup step does it, and a reader sees the old or the new value.
        unsafe { std::env::set_var("PATH", path) };
    }
}

/// Whether this Windows can make Dev Drives (Windows 11 22H2, build 22621, and later).
pub fn dev_drive_supported() -> bool {
    #[cfg(windows)]
    {
        let out = command("cmd.exe").args(["/D", "/C", "ver"]).output();
        // `Microsoft Windows [Version 10.0.26100.6584]`
        out.ok()
            .and_then(|o| {
                let t = String::from_utf8_lossy(&o.stdout).into_owned();
                let v = t
                    .split("Version")
                    .nth(1)?
                    .trim()
                    .trim_end_matches(']')
                    .to_string();
                v.split('.').nth(2)?.parse::<u32>().ok()
            })
            .is_some_and(|build| build >= 22621)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Whether `path` is on a Dev Drive, where Defender scans without holding up the
/// programs reading files; `None` when it can't be told (or not on Windows). Asked of the
/// volume itself, so it doesn't depend on Windows' language as `fsutil`'s text would.
pub fn on_dev_drive(path: &Path) -> Option<bool> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, GetVolumePathNameW, OPEN_EXISTING,
        };
        use windows_sys::Win32::System::IO::DeviceIoControl;
        use windows_sys::Win32::System::Ioctl::{
            FILE_FS_PERSISTENT_VOLUME_INFORMATION, FSCTL_QUERY_PERSISTENT_VOLUME_STATE,
            PERSISTENT_VOLUME_STATE_DEV_VOLUME,
        };
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut root = [0u16; 512];
        // SAFETY: plain Win32 calls; every buffer is sized as passed and outlives the call,
        // and the handle is closed here.
        unsafe {
            if GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0 {
                return None;
            }
            let h = CreateFileW(
                root.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                return None;
            }
            let ask = FILE_FS_PERSISTENT_VOLUME_INFORMATION {
                FlagMask: PERSISTENT_VOLUME_STATE_DEV_VOLUME,
                Version: 1,
                ..Default::default()
            };
            let mut got = FILE_FS_PERSISTENT_VOLUME_INFORMATION::default();
            let mut len = 0u32;
            let size = size_of::<FILE_FS_PERSISTENT_VOLUME_INFORMATION>() as u32;
            let ok = DeviceIoControl(
                h,
                FSCTL_QUERY_PERSISTENT_VOLUME_STATE,
                (&ask as *const FILE_FS_PERSISTENT_VOLUME_INFORMATION).cast(),
                size,
                (&mut got as *mut FILE_FS_PERSISTENT_VOLUME_INFORMATION).cast(),
                size,
                &mut len,
                std::ptr::null_mut(),
            ) != 0;
            CloseHandle(h);
            // NTFS doesn't know the question: not a Dev Drive (they are ReFS).
            Some(ok && got.VolumeFlags & PERSISTENT_VOLUME_STATE_DEV_VOLUME != 0)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// The account's home folder when the environment names none.
pub fn account_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        nix::unistd::User::from_uid(nix::unistd::getuid())
            .ok()
            .flatten()
            .map(|u| u.dir)
    }
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
    }
}

/// Bytes a file takes on disk, and for a file with more than one name the key that
/// tells its names apart from other files' (`(device, inode)`; none on Windows, where
/// std doesn't expose it, so hard links count once per name there).
pub fn disk_usage(m: &fs::Metadata) -> (u64, Option<(u64, u64)>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let shared = !m.is_dir() && m.nlink() > 1;
        (m.blocks() * 512, shared.then(|| (m.dev(), m.ino())))
    }
    #[cfg(windows)]
    {
        (m.len(), None)
    }
}

/// `HH:MM` in local time, for a time in seconds since 1970.
pub fn local_clock(at: u64) -> String {
    #[cfg(unix)]
    {
        let t = at as nix::libc::time_t;
        // SAFETY: `localtime_r` only writes into `tm`, which lives on this stack.
        let mut tm: nix::libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { nix::libc::localtime_r(&t, &mut tm) }.is_null() {
            return String::from("--:--");
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{
            FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime,
        };
        // Seconds since 1970 to 100 ns ticks since 1601.
        let ticks = (at + 11_644_473_600) * 10_000_000;
        let ft = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        // SAFETY: both calls only write into the SYSTEMTIMEs on this stack.
        unsafe {
            let (mut utc, mut local): (SYSTEMTIME, SYSTEMTIME) =
                (std::mem::zeroed(), std::mem::zeroed());
            if FileTimeToSystemTime(&ft, &mut utc) == 0
                || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
            {
                return String::from("--:--");
            }
            format!("{:02}:{:02}", local.wHour, local.wMinute)
        }
    }
}

/// The oldest macOS each download runs on, by kind and the first major version it
/// holds for. The numbers are the binaries' `minos` (`vtool -show-build`), read on
/// arm64 copies: mysqld 8.4.11 (mysql-…-macos15 tarball) 14.0, postgres 18.6.0
/// 15.0, mailpit 1.31.6 13.0, node 24.22.0 13.5 (22.23.3: 11.0).
/// Not measured, assumed the same: MySQL 9.7 and 8.0 (the same macos15 tarballs),
/// PostgreSQL 14–17 (theseus-rs builds every version on GitHub's macos-15 runners,
/// its targets.json) and Node.js 25+ (nodejs.org's BUILDING.md says 13.5 from 24 on).
const MIN_MACOS: [(&str, u32, (u32, u32)); 4] = [
    ("mysql", 0, (14, 0)),
    ("postgres", 0, (15, 0)),
    ("mailpit", 0, (13, 0)),
    ("node", 24, (13, 5)),
];

/// "14.0" → (14, 0); "13.6.1" → (13, 6).
fn os_version(v: &str) -> Option<(u32, u32)> {
    let mut p = v.trim().split('.').map(|n| n.parse::<u32>().ok());
    Some((p.next()??, p.next().flatten().unwrap_or(0)))
}

fn show_os((major, minor): (u32, u32)) -> String {
    if minor == 0 {
        major.to_string()
    } else {
        format!("{major}.{minor}")
    }
}

/// Why `name version` (a service kind or runtime) can't run on macOS `have`, if it can't.
fn too_old(name: &str, kind: &str, version: &str, have: &str) -> Option<String> {
    let major: u32 = version.split('.').next()?.parse().unwrap_or(0);
    let need = MIN_MACOS
        .iter()
        .find(|(k, from, _)| *k == kind && major >= *from)?
        .2;
    (os_version(have)? < need).then(|| {
        format!(
            "{name} {version} needs macOS {} or later; this Mac has {}.",
            show_os(need),
            have.trim()
        )
    })
}

/// This Mac's macOS version ("13.6.1"); None elsewhere.
fn macos() -> Option<&'static str> {
    static V: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = command("sysctl")
            .args(["-n", "kern.osproductversion"])
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|v| !v.is_empty())
    })
    .as_deref()
}

/// Why a download won't run on this Mac, said before it is fetched: "MySQL 8.4.11
/// needs macOS 14 or later; this Mac has 13.6." None when it runs (and off macOS).
pub fn needs_newer_macos(name: &str, kind: &str, version: &str) -> Option<String> {
    too_old(name, kind, version, macos()?)
}

#[cfg(test)]
mod tests {
    use super::{netstat_listener, program_under, quote_arg, tasklist_program};

    #[cfg(windows)]
    #[test]
    fn tcp_listening_reads_ipv4_and_ipv6_listeners() {
        let v4 = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let v6 = std::net::TcpListener::bind("[::1]:0").unwrap();
        let (p4, p6) = (
            v4.local_addr().unwrap().port(),
            v6.local_addr().unwrap().port(),
        );
        assert_eq!(super::tcp_listening(p4), Some(true));
        assert_eq!(super::tcp_listening(p6), Some(true));
        drop(v4);
        assert_eq!(super::tcp_listening(p4), Some(false));
    }

    #[test]
    fn windows_arguments_stay_one_argument() {
        assert_eq!(quote_arg("-D"), "-D");
        assert_eq!(quote_arg(r"C:\dir\file"), r"C:\dir\file");
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(
            quote_arg(r"C:\Program Files\pg"),
            r#""C:\Program Files\pg""#
        );
        // A trailing backslash must not escape the closing quote.
        assert_eq!(quote_arg(r"C:\My Dir\"), r#""C:\My Dir\\""#);
        assert_eq!(quote_arg(r#"a "b" c"#), r#""a \"b\" c""#);
        assert_eq!(quote_arg(r#"a\" b"#), r#""a\\\" b""#);
    }

    #[test]
    fn an_app_is_found_by_its_folder_and_the_newest_version_wins() {
        let root = tempfile::tempdir().unwrap();
        for v in ["PhpStorm 2024.1", "PhpStorm 2025.2"] {
            let bin = root.path().join("JetBrains").join(v).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("phpstorm64.exe"), "").unwrap();
        }
        let pattern = "JetBrains\\PhpStorm*\\bin\\phpstorm64.exe";
        let found = program_under(root.path(), pattern).unwrap();
        assert!(found.to_string_lossy().contains("2025.2"), "{found:?}");
        assert_eq!(program_under(root.path(), "Fork\\Fork.exe"), None);
    }

    #[test]
    fn netstat_finds_the_listener_whatever_the_language() {
        let out = "  Proto  Local  Foreign  State  PID\n  \
                   TCP    127.0.0.1:50443  10.0.0.1:443   ESTABLISHED  7\n  \
                   TCP    10.0.0.2:51000   10.0.0.1:443   ESTABLISHED  8\n  \
                   TCP    0.0.0.0:443      0.0.0.0:0      DİNLİYOR     24460\n";
        assert_eq!(netstat_listener(out, 443), Some(24460));
        assert_eq!(netstat_listener(out, 80), None);
        let two_words = "  TCP    0.0.0.0:443      0.0.0.0:0      EN ECOUTE    24460\n";
        assert_eq!(netstat_listener(two_words, 443), Some(24460));
    }

    #[test]
    fn tasklist_names_the_program_and_explains_wsl() {
        assert_eq!(
            tasklist_program("\"caddy.exe\",\"12\",\"Console\",\"1\",\"9 K\""),
            Some("caddy.exe".into())
        );
        assert!(
            tasklist_program("\"wslrelay.exe\",\"3\"")
                .unwrap()
                .contains("WSL")
        );
        assert_eq!(tasklist_program("INFO: No tasks are running."), None);
    }

    #[test]
    fn downloads_too_new_for_this_macos_are_refused_with_the_reason() {
        use super::too_old;
        assert_eq!(
            too_old("MySQL", "mysql", "8.4.11", "13.6.1").as_deref(),
            Some("MySQL 8.4.11 needs macOS 14 or later; this Mac has 13.6.1.")
        );
        assert_eq!(too_old("MySQL", "mysql", "8.4.11", "14.0"), None);
        assert_eq!(too_old("MySQL", "mysql", "9.7.2", "15.2"), None);
        assert!(too_old("PostgreSQL", "postgres", "16.15.0", "14.7").is_some());
        assert_eq!(too_old("PostgreSQL", "postgres", "18.6.0", "26.0"), None);
        assert!(too_old("Mailpit", "mailpit", "1.31.6", "12.7.6").is_some());
        assert_eq!(too_old("Mailpit", "mailpit", "1.31.6", "13"), None);
        assert_eq!(
            too_old("Node.js", "node", "24", "13.4").as_deref(),
            Some("Node.js 24 needs macOS 13.5 or later; this Mac has 13.4.")
        );
        assert_eq!(too_old("Node.js", "node", "24", "13.5"), None);
        assert!(too_old("Node.js", "node", "26", "12.7").is_some());
        assert_eq!(too_old("Node.js", "node", "22", "12.0"), None);
        // Kinds without a floor, and versions macOS can't be read for, pass.
        assert_eq!(too_old("Redis", "redis", "8.10.2", "12.0"), None);
        assert_eq!(too_old("MySQL", "mysql", "8.4.11", ""), None);
    }

    #[test]
    fn left_groups_walks_by_parent_and_keeps_only_who_left() {
        let row = |pid, ppid, pgid, c: &str| (pid, ppid, pgid, c.to_string());
        let table = [
            row(1, 0, 1, "launchd"),
            row(100, 50, 50, "anvil"),
            // The app: a shell leading group 200, node in it, a flusher in its own session
            // and that one's child, also outside.
            row(200, 100, 200, "sh -c npm run dev"),
            row(201, 200, 200, "node next dev"),
            row(202, 201, 202, "node detached-flush.js"),
            row(203, 202, 202, "node child-of-flush"),
            // A member whose parent already ended (adopted) still counts as the group's.
            row(204, 1, 200, "node worker"),
            row(205, 204, 300, "node worker-child"),
            // A stranger in its own group, and an unrelated orphan, are not descendants.
            row(300, 1, 300, "other"),
            row(400, 1, 400, "node detached-flush.js"),
        ];
        let mut got = crate::platform::left_groups(&table, &[200]);
        got.sort();
        assert_eq!(
            got,
            vec![
                (202, "node detached-flush.js".into()),
                (203, "node child-of-flush".into()),
                (205, "node worker-child".into()),
            ]
        );
        // A pid that leads no group (a plain child) has no members: nothing.
        assert!(crate::platform::left_groups(&table, &[100]).is_empty());
    }
}
