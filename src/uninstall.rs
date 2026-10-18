//! `anvil uninstall`: undo what Anvil did to this Mac. Lists everything first;
//! only `--yes` removes it. Anything not recognisably Anvil's is left alone.

use crate::style::{bold, commands, error, yellow};
use crate::{Config, PF_ANCHOR, PORTS_LABEL, SYSTEM_KEYCHAIN, home, run, shims};
use anyhow::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub enum Step {
    /// `/etc/resolver/<tld>` pointing at Anvil's DNS server (needs sudo).
    Resolver(PathBuf),
    /// The LaunchDaemon forwarding ports 80 and 443, and its pf rules (needs sudo).
    Ports(PathBuf),
    /// Anvil's certificate authority in the login keychain, by SHA-1.
    Authority(String),
    /// The same in the System keychain (needs sudo).
    SystemAuthority(String),
    /// The lines in a shell rc file that put Anvil's shims on PATH.
    PathBlock(PathBuf),
    /// A file or folder Anvil or its app made.
    Remove(PathBuf),
    /// Anvil's home folder. Last, and skipped when an earlier step failed:
    /// the certificate and the DNS port are only found through it.
    Home(PathBuf),
}

impl Step {
    pub fn describe(&self) -> String {
        match self {
            Step::Resolver(p) | Step::Ports(p) => {
                format!("{} (asks for your password)", p.display())
            }
            Step::Authority(_) => {
                "Anvil's HTTPS certificate authority in your login keychain".into()
            }
            Step::SystemAuthority(_) => {
                "Anvil's HTTPS certificate authority in the System keychain (asks for your password)"
                    .into()
            }
            Step::PathBlock(p) => format!("Anvil's PATH lines in {}", tilde(p)),
            Step::Remove(p) | Step::Home(p) => tilde(p),
        }
    }
}

fn tilde(p: &Path) -> String {
    match p.strip_prefix(crate::user_home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

/// Exactly what Anvil writes: `nameserver 127.0.0.1` and its DNS port. A
/// resolver file with anything else in it belongs to someone else.
pub fn is_anvil_resolver(text: &str, dns_port: u16) -> bool {
    let port = dns_port.to_string();
    text.split_whitespace()
        .eq(["nameserver", "127.0.0.1", "port", port.as_str()])
}

/// SHA-1 of a PEM certificate, as `security` prints it.
fn fingerprint(cert: &Path) -> Option<String> {
    let out = Command::new("openssl")
        .args(["x509", "-noout", "-fingerprint", "-sha1", "-in"])
        .arg(cert)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.split_once('=')?.1.trim().replace(':', ""))
}

fn login_keychain() -> PathBuf {
    crate::user_home().join("Library/Keychains/login.keychain-db")
}

/// Everything there is to undo, in the order it has to happen: the
/// certificate is read from ~/.anvil, so the folder goes last.
pub fn plan(cfg: &Config) -> Vec<Step> {
    let mut steps = Vec::new();

    let root = crate::root_cert();
    if let Some(sha1) = fingerprint(&root) {
        let listed = |keychain: &Path| {
            Command::new("security")
                .args(["find-certificate", "-a", "-Z"])
                .arg(keychain)
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&sha1))
        };
        if listed(&login_keychain()) {
            steps.push(Step::Authority(sha1.clone()));
        }
        if listed(Path::new(SYSTEM_KEYCHAIN)) {
            steps.push(Step::SystemAuthority(sha1));
        }
    }
    for f in shims::rc_files_with_block() {
        steps.push(Step::PathBlock(f));
    }

    // Every tld Anvil was set up for, not only the current one.
    for e in fs::read_dir("/etc/resolver")
        .into_iter()
        .flatten()
        .flatten()
    {
        if fs::read_to_string(e.path()).is_ok_and(|t| is_anvil_resolver(&t, cfg.dns_port)) {
            steps.push(Step::Resolver(e.path()));
        }
    }

    let daemon = PathBuf::from(format!("/Library/LaunchDaemons/{PORTS_LABEL}.plist"));
    if daemon.exists() {
        steps.push(Step::Ports(daemon));
    }

    let user = crate::user_home();
    // The login item (tauri-plugin-autostart names it after the app), only if it starts Anvil.
    // It starts the app's binary, anvil-app, inside Anvil.app or a development build.
    let agent = user.join("Library/LaunchAgents/Anvil.plist");
    if fs::read_to_string(&agent).is_ok_and(|t| t.contains("anvil-app")) {
        steps.push(Step::Remove(agent));
    }
    // What macOS and the app's web view keep: by bundle id, or by binary name
    // for a development build.
    for p in [
        "Library/WebKit/anvil-app",
        "Library/Caches/anvil-app",
        "Library/WebKit/dev.anvil.desktop",
        "Library/Caches/dev.anvil.desktop",
        "Library/Application Support/dev.anvil.desktop",
        "Library/Saved Application State/dev.anvil.desktop.savedState",
        "Library/Preferences/dev.anvil.desktop.plist",
    ] {
        let p = user.join(p);
        if p.symlink_metadata().is_ok() {
            steps.push(Step::Remove(p));
        }
    }

    let h = home();
    if h.symlink_metadata().is_ok() {
        steps.push(Step::Home(h));
    }
    steps
}

/// What Anvil creates in its home folder.
const ANVIL_ENTRIES: [&str; 13] = [
    "bin",
    "build",
    "cache",
    "composer",
    "config.toml",
    "data",
    "log",
    "php",
    "run",
    "runtimes",
    "services",
    "shims",
    ".DS_Store",
];

/// Why `h` must not be deleted, if anything. ANVIL_HOME can point anywhere, so
/// only a folder holding nothing but what Anvil makes, with no project inside, goes.
pub fn home_problem(h: &Path, user: &Path, projects: &[PathBuf]) -> Option<String> {
    if !h.is_absolute() {
        return Some("it isn't an absolute path".into());
    }
    if h.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Some("it is a link; remove the folder it points to yourself".into());
    }
    let Ok(h) = h.canonicalize() else {
        return Some("it can't be read".into());
    };
    if user.canonicalize().is_ok_and(|u| u.starts_with(&h)) {
        return Some("it is your home folder or holds it".into());
    }
    // Something only Anvil makes must be there, not just nothing foreign.
    if !["config.toml", "run", "data"]
        .iter()
        .any(|e| h.join(e).exists())
    {
        return Some("it has none of Anvil's own files".into());
    }
    let Ok(entries) = fs::read_dir(&h) else {
        return Some("it can't be read".into());
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let tmp = name.starts_with("config.toml.") && name.ends_with(".tmp");
        if !ANVIL_ENTRIES.contains(&name.as_str()) && !tmp {
            return Some(format!("it holds {name}, which Anvil didn't make"));
        }
    }
    projects
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .find(|p| p.starts_with(&h))
        .map(|p| format!("your project folder {} is inside it", p.display()))
}

fn projects(cfg: &Config) -> Vec<PathBuf> {
    cfg.paths
        .iter()
        .chain(cfg.links.values())
        .cloned()
        .collect()
}

/// The app, `anvil start`, and anything running from Anvil's home (servers,
/// services, runtimes). Apps Anvil started stop with the app or `anvil start`.
fn still_running() -> Result<Vec<String>> {
    let out = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()?;
    if !out.status.success() {
        bail!("Couldn't list running programs to check Anvil is stopped");
    }
    let me = std::process::id().to_string();
    let h = home().display().to_string();
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter_map(|l| l.split_once(' '))
        .filter(|(pid, _)| *pid != me)
        .filter(|(_, cmd)| anvil_process(cmd, &h))
        .map(|(_, cmd)| cmd.to_string())
        .collect())
}

fn anvil_process(cmd: &str, home: &str) -> bool {
    let mut words = cmd.split_whitespace();
    let exe = words
        .next()
        .map_or("", |w| w.rsplit('/').next().unwrap_or(w));
    exe == "anvil-app" || (exe == "anvil" && words.next() == Some("start")) || cmd.contains(home)
}

/// `config_ok` false: config.toml couldn't be read, so the projects that must
/// not be inside ~/.anvil are unknown and it is kept.
pub fn uninstall(cfg: &Config, config_ok: bool, yes: bool) -> Result<()> {
    let h = home();
    let user = crate::user_home();
    let problem = if config_ok {
        home_problem(&h, &user, &projects(cfg))
    } else {
        Some("its config.toml couldn't be read, so Anvil can't tell where your projects are".into())
    };
    let steps: Vec<Step> = plan(cfg)
        .into_iter()
        .filter(|s| !(matches!(s, Step::Home(_)) && problem.is_some()))
        .collect();
    if let Some(why) = &problem {
        println!("{}\n", yellow(format!("{} is kept: {why}.", tilde(&h))));
    }
    if steps.is_empty() {
        println!("Nothing of Anvil's is left on this Mac.");
        return Ok(());
    }
    println!("{}\n", bold("Anvil uninstall removes:"));
    for s in &steps {
        println!("  {}", s.describe());
    }
    if steps.iter().any(|s| matches!(s, Step::Home(_))) {
        println!(
            "\n{}",
            yellow(format!(
                "{} holds every PHP, runtime and service Anvil installed, and the\n\
                 databases of your services (data/). Back those up first if you need them.",
                tilde(&h)
            ))
        );
    }
    println!(
        "\n{}",
        commands(
            "Your projects are not touched. Anvil.app and the anvil command itself stay:\n\
             drag Anvil.app to the Trash, and `cargo uninstall anvil` if you installed it with cargo."
        )
    );
    if !yes {
        println!("\n{}", commands("Run `anvil uninstall --yes` to do it."));
        return Ok(());
    }

    let running = still_running()?;
    if !running.is_empty() {
        bail!(
            "Quit Anvil first; still running:\n  {}",
            running.join("\n  ")
        );
    }

    println!();
    apply(&steps);
    Ok(())
}

/// Do each step; a failure is reported and the rest still run, so the next
/// `anvil uninstall` lists only what is left.
fn apply(steps: &[Step]) {
    let mut failed = false;
    for s in steps {
        if matches!(s, Step::Home(_)) && failed {
            println!(
                "{}",
                commands(&format!(
                    "Kept {}: run `anvil uninstall` again once the rest is gone.",
                    s.describe()
                ))
            );
            continue;
        }
        let done = match s {
            Step::Authority(sha1) => run(Command::new("security")
                .args(["delete-certificate", "-Z", sha1, "-t"])
                .arg(login_keychain())),
            Step::SystemAuthority(sha1) => {
                // The admin-domain trust setting outlives the certificate unless it is
                // removed first; the root file is still in ~/.anvil at this point.
                let root = home().join("data/caddy/pki/authorities/local/root.crt");
                let _ = run(Command::new("sudo")
                    .args(["security", "remove-trusted-cert", "-d"])
                    .arg(&root));
                run(Command::new("sudo").args([
                    "security",
                    "delete-certificate",
                    "-Z",
                    sha1,
                    "-t",
                    SYSTEM_KEYCHAIN,
                ]))
            }
            Step::PathBlock(p) => shims::remove_path_block(p),
            Step::Resolver(p) => run(Command::new("sudo").arg("rm").arg("-f").arg(p)),
            Step::Ports(p) => run(Command::new("sudo").args([
                "sh",
                "-c",
                &format!(
                    "launchctl bootout system/{PORTS_LABEL}; rm -f '{}' && pfctl -a {PF_ANCHOR} -F all",
                    p.display()
                ),
            ])),
            // A link is removed, never what it points to.
            Step::Remove(p) | Step::Home(p) => p
                .symlink_metadata()
                .and_then(|m| {
                    if m.is_dir() {
                        fs::remove_dir_all(p)
                    } else {
                        fs::remove_file(p)
                    }
                })
                .map_err(Into::into),
        };
        match done {
            Ok(()) => println!("Removed {}", s.describe()),
            Err(e) => {
                failed = true;
                eprintln!(
                    "{}",
                    error(format!("Couldn't remove {}: {e:#}", s.describe()))
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_anvils_resolver_files() {
        assert!(is_anvil_resolver("nameserver 127.0.0.1\nport 5354\n", 5354));
        assert!(is_anvil_resolver("nameserver 127.0.0.1\nport 5354", 5354));
        // Another DNS port, another tool's file, or extra lines: not ours.
        assert!(!is_anvil_resolver("nameserver 127.0.0.1\nport 53\n", 5354));
        assert!(!is_anvil_resolver("nameserver 127.0.0.1\n", 5354));
        assert!(!is_anvil_resolver(
            "nameserver 127.0.0.1\nport 5354\nsearch_order 1\n",
            5354
        ));
        assert!(!is_anvil_resolver("", 5354));
    }

    #[test]
    fn removes_links_not_their_targets() {
        let dir = std::env::temp_dir().join(format!("anvil-uninstall-{}", std::process::id()));
        let keep = dir.join("keep");
        fs::create_dir_all(keep.join("inner")).unwrap();
        fs::write(keep.join("inner/file"), "x").unwrap();
        let gone = dir.join("gone");
        fs::create_dir_all(gone.join("deep")).unwrap();
        std::os::unix::fs::symlink(&keep, dir.join("link")).unwrap();
        fs::write(dir.join("file.plist"), "x").unwrap();
        apply(&[
            Step::Remove(gone.clone()),
            Step::Remove(dir.join("link")),
            Step::Remove(dir.join("missing")), // reported, the rest still run
            Step::Remove(dir.join("file.plist")),
        ]);
        assert!(!gone.exists());
        assert!(dir.join("link").symlink_metadata().is_err());
        assert!(!dir.join("file.plist").exists());
        assert_eq!(fs::read_to_string(keep.join("inner/file")).unwrap(), "x");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deletes_only_an_anvil_folder() {
        let base = std::env::temp_dir().join(format!("anvil-home-{}", std::process::id()));
        let h = base.join("anvil");
        fs::create_dir_all(h.join("data")).unwrap();
        fs::write(h.join("config.toml"), "").unwrap();
        let user = base.join("me");
        fs::create_dir_all(&user).unwrap();
        assert_eq!(home_problem(&h, &user, &[]), None);
        // A project parked inside it, or something Anvil didn't make, keeps it.
        fs::create_dir_all(h.join("data/site")).unwrap();
        assert!(home_problem(&h, &user, &[h.join("data/site")]).is_some());
        fs::create_dir_all(h.join("Sites")).unwrap();
        assert!(home_problem(&h, &user, &[]).unwrap().contains("Sites"));
        fs::remove_dir_all(h.join("Sites")).unwrap();
        // Relative paths, the home folder and anything above it are refused.
        assert!(home_problem(Path::new("."), &user, &[]).is_some());
        assert!(home_problem(&user, &user, &[]).is_some());
        assert!(home_problem(&base, &user, &[]).is_some());
        assert!(home_problem(Path::new("/"), &user, &[]).is_some());
        // Nothing foreign is not enough: a folder with only bin/ isn't Anvil's.
        let bin_only = base.join("local");
        fs::create_dir_all(bin_only.join("bin")).unwrap();
        assert!(home_problem(&bin_only, &user, &[]).is_some());
        // A link to it is not followed.
        std::os::unix::fs::symlink(&h, base.join("link")).unwrap();
        assert!(home_problem(&base.join("link"), &user, &[]).is_some());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn anvil_processes() {
        let h = "/Users/me/.anvil";
        assert!(anvil_process(
            "/Applications/Anvil.app/Contents/MacOS/anvil-app",
            h
        ));
        assert!(anvil_process("anvil start", h));
        assert!(anvil_process(
            "/Users/me/.anvil/php/8.4/php-fpm --nodaemonize",
            h
        ));
        assert!(!anvil_process("anvil uninstall --yes", h));
        assert!(!anvil_process("/usr/bin/vim notes-about-anvil", h));
    }

    #[test]
    fn home_waits_for_failed_steps() {
        let dir = std::env::temp_dir().join(format!("anvil-order-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        apply(&[Step::Remove(dir.join("missing")), Step::Home(dir.clone())]);
        assert!(dir.exists());
        apply(&[Step::Home(dir.clone())]);
        assert!(!dir.exists());
    }
}
