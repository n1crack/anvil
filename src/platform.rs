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

/// Open a file, folder or web address as the desktop does: macOS `open`; on Windows the
/// program registered for it.
pub fn open(target: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        // Exits 0 (explorer.exe exits 1 on success) and takes any URL as one argument.
        let mut c = command("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler").arg(target);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = command("open");
        c.arg(target);
        c
    }
}

/// Open `target` with the application `app`: macOS `open -a`; on Windows `app` is the
/// program to run (`code`, a full path).
pub fn open_with(app: &str, target: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        let mut c = command("cmd.exe");
        // `start` finds programs the way the Run dialog does (App Paths), and returns.
        c.args(["/D", "/C", "start", "\"\"", app]).arg(target);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = command("open");
        c.args(["-a", app]).arg(target);
        c
    }
}

/// Show a file in the file manager: selected in Finder; on Windows its folder.
pub fn reveal(file: &Path) -> Command {
    if cfg!(windows) {
        open(file.parent().unwrap_or(file))
    } else {
        let mut c = command("open");
        c.arg("-R").arg(file);
        c
    }
}

/// A terminal window in `dir`: `app` when set (macOS `open -a`, a program on Windows),
/// else Terminal, or on Windows Windows Terminal, else PowerShell in a console.
pub fn terminal(app: Option<&str>, dir: &Path) -> Command {
    #[cfg(windows)]
    {
        if let Some(app) = app {
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
    #[cfg(not(windows))]
    {
        open_with(app.unwrap_or("Terminal"), dir)
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

/// Process-group leaders whose parent is gone (launchd adopted them), with their
/// command lines: what a killed Anvil's dev shells become.
// ponytail: "parent gone" is ppid 1; Linux with a subreaper (systemd --user) reparents
// elsewhere and these are missed there, pid files still cover them.
#[cfg(unix)]
pub fn orphan_leaders() -> Vec<(u32, String)> {
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
            let pid: u32 = pid.parse().ok()?;
            (ppid == "1" && pgid.parse() == Ok(pid)).then(|| (pid, command.trim().to_string()))
        })
        .collect()
}

/// Windows: no tag on the command line to recognise (`shell`); pid files only.
#[cfg(windows)]
pub fn orphan_leaders() -> Vec<(u32, String)> {
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
