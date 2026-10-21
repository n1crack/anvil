//! Terminal shims: `php`, `composer`, `laravel`, `node`, `npm`, `npx` and `pnpm` in
//! `~/.anvil/shims` run `anvil shim <tool>`, which picks the version of the site
//! the current folder belongs to.

use crate::{Config, Site, apps, create, home, installed_php, run, runtimes, sites};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub const TOOLS: [&str; 7] = ["php", "composer", "laravel", "node", "npm", "npx", "pnpm"];

const BLOCK_START: &str = "# Anvil: php, composer, laravel and node follow each site's versions";
const BLOCK_END: &str = "# Anvil end";

/// `'…'` for sh, whatever the text holds.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The anvil CLI the shims run: this program when it is the CLI; from the app, the
/// CLI bundled next to it, else cargo's, else whatever `anvil` is on PATH.
pub fn pick_cli(exe: &Path, user_home: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if exe.file_stem().is_some_and(|n| n == "anvil") {
        return exe.to_path_buf();
    }
    [
        exe.with_file_name(crate::platform::exe("anvil")),
        user_home.join(".cargo/bin/anvil"),
    ]
    .into_iter()
    .find(|p| exists(p))
    .unwrap_or_else(|| "anvil".into())
}

use crate::user_home;

/// Write the shims into `<home>/shims`.
#[cfg(windows)]
pub fn install(home: &Path) -> Result<()> {
    // `<tool>.exe` is the CLI itself under the tool's name (a hard link, else a copy):
    // the CLI sees its name and acts as the shim. A .cmd would ask "Terminate batch
    // job?" on Ctrl+C and mangle quoted arguments.
    let exe = std::env::current_exe().unwrap_or_default();
    let cli = pick_cli(&exe, &user_home(), Path::exists);
    if !cli.is_absolute() {
        bail!("couldn't find anvil.exe to make the shims from");
    }
    let dir = home.join("shims");
    fs::create_dir_all(&dir)?;
    for tool in TOOLS.iter().copied().chain(["anvil"]) {
        let file = dir.join(format!("{tool}.exe"));
        if file == cli {
            continue;
        }
        // A running shim can't be replaced; it is the same program, so keep it.
        if fs::remove_file(&file).is_err() && file.exists() {
            continue;
        }
        if fs::hard_link(&cli, &file).is_err() {
            fs::copy(&cli, &file)?;
        }
    }
    Ok(())
}

/// The tool a shim stands for when this program runs under its name (`php.exe`).
pub fn shim_name(exe: &Path) -> Option<&'static str> {
    let stem = exe.file_stem()?.to_str()?;
    TOOLS.iter().copied().find(|t| t.eq_ignore_ascii_case(stem))
}

/// Write the shims into `<home>/shims`.
#[cfg(unix)]
pub fn install(home: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let exe = std::env::current_exe().unwrap_or_default();
    let cli = pick_cli(&exe, &user_home(), Path::exists);
    let dir = home.join("shims");
    fs::create_dir_all(&dir)?;
    for tool in TOOLS {
        let file = dir.join(tool);
        fs::write(
            &file,
            format!(
                "#!/bin/sh\nexec {} shim {tool} \"$@\"\n",
                sh_quote(&cli.to_string_lossy())
            ),
        )?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
    }
    // `anvil` itself, so the CLI inside the app works in a terminal. Only a known
    // path: a bare "anvil" would run this shim again.
    if cli.is_absolute() {
        let file = dir.join("anvil");
        fs::write(
            &file,
            format!(
                "#!/bin/sh\nexec {} \"$@\"\n",
                sh_quote(&cli.to_string_lossy())
            ),
        )?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The site whose folder holds `cwd`, the innermost when folders nest. Links
/// are resolved: macOS reaches /var through /private/var. Of several sites on one folder
/// (an alias), one with extensions of its own, so the terminal loads what its web does.
pub fn site_for<'a>(sites: &'a [Site], cwd: &Path) -> Option<&'a Site> {
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let cwd = real(cwd);
    sites
        .iter()
        .map(|s| (s, real(&s.path)))
        .filter(|(_, p)| cwd.starts_with(p))
        .max_by_key(|(s, p)| (p.components().count(), !s.ext.is_empty()))
        .map(|(s, _)| s)
}

/// The site's own PHP version (the user's, else `.anvil.toml`'s), else the default, else the newest installed.
fn php_version(pinned: Option<String>, cfg: &Config, installed: &[String]) -> Result<String> {
    match pinned.or_else(|| cfg.php.clone()) {
        Some(v) if installed.contains(&v) => Ok(v),
        Some(v) => bail!("PHP {v} isn't installed in Anvil: run `anvil php install {v}`"),
        None => installed
            .last()
            .cloned()
            .context("No PHP installed in Anvil: run `anvil php install 8.4`"),
    }
}

fn path_with(dir: &Path) -> (String, String) {
    ("PATH".into(), crate::prepend_path(dir, &crate::env_path()))
}

pub type Resolved = (PathBuf, Vec<String>, Vec<(String, String)>);

/// What `tool` runs in `cwd`: the program, the arguments before the user's, and
/// extra environment. `fetch` downloads Composer or the Laravel installer when missing,
/// and makes a worktree's own database on its first use.
pub fn resolve(tool: &str, cwd: &Path, cfg: &Config) -> Result<Resolved> {
    resolve_with(tool, cwd, cfg, true)
}

pub fn resolve_with(tool: &str, cwd: &Path, cfg: &Config, fetch: bool) -> Result<Resolved> {
    resolve_run(tool, &[], cwd, cfg, fetch)
}

/// `resolve` for a shim run with `args`: a test run (`is_test_run`) gets the project's
/// test database when it isn't main's (`worktrees::checked_env`).
pub fn resolve_run(
    tool: &str,
    args: &[String],
    cwd: &Path,
    cfg: &Config,
    fetch: bool,
) -> Result<Resolved> {
    let all = sites(cfg);
    let site = site_for(&all, cwd);
    // First: a worktree Anvil can't separate from main runs nothing, whatever the tool.
    known_worktree(cfg, site, cwd)?;
    let extra = extra_env(cfg, site, tool, args)?;
    if fetch && let Some(s) = site.filter(|s| s.worktree_of.is_some()) {
        // Its own database on first use, as a Start would; the line shows while it's made.
        // Refused when that fails: never run against a missing or half-made database.
        crate::worktrees::prepare(&mut crate::load()?, s, &mut |line| eprintln!("{line}"))?;
    }
    let mut r = resolve_for(tool, cwd, cfg, fetch, site)?;
    r.2.extend(extra);
    Ok(r)
}

/// `php artisan test`, `php vendor/bin/phpunit` (or pest, paratest, Symfony's `bin/phpunit`; `./vendor/bin/pest`
/// reaches the shim the same way, through its `#!/usr/bin/env php`). Only to give it a test
/// config's own database: a test run this misses (`composer test`, a named script) keeps
/// the worktree's own database, never main's.
fn is_test_run(tool: &str, args: &[String]) -> bool {
    if tool != "php" {
        return false;
    }
    let mut words = args.iter().map(String::as_str);
    let mut script = None;
    while let Some(w) = words.next() {
        match w {
            "-d" | "-c" => {
                words.next();
            }
            w if w.starts_with('-') => {}
            w => {
                script = Some(w);
                break;
            }
        }
    }
    let Some(script) = script.map(Path::new) else {
        return false;
    };
    if script.file_name().is_some_and(|n| n == "artisan") {
        return words.next() == Some("test");
    }
    // Symfony's phpunit-bridge: `bin/phpunit`, `vendor/bin/simple-phpunit`.
    script.ends_with("bin/phpunit")
        || ["pest", "paratest", "simple-phpunit"]
            .iter()
            .any(|t| script.ends_with(Path::new("vendor/bin").join(t)))
}

/// Refuses a folder in a git worktree of an Anvil app (a hidden one too) that isn't
/// that worktree's site: it would run with no worktree env, on main's database.
fn known_worktree(cfg: &Config, site: Option<&Site>, cwd: &Path) -> Result<()> {
    let cwd = crate::platform::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let Some(root) = cwd.ancestors().find(|d| d.join(".git").exists()) else {
        return Ok(());
    };
    let Some(main) = crate::worktrees::main_of(root) else {
        return Ok(()); // not a linked worktree
    };
    let real = |p: &Path| crate::platform::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    if site.is_some_and(|s| real(&s.path).starts_with(root)) {
        return Ok(());
    }
    if crate::app_folders(cfg).contains(&main) {
        bail!(
            "Anvil can't tell which worktree {} is: it belongs to {} but isn't one of its worktrees in Anvil",
            root.display(),
            main.display()
        );
    }
    Ok(())
}

/// A worktree's own database and prefixes (`worktrees::checked_env`), so a terminal
/// `php artisan migrate` stays off the main app's; a test run's test database; the mail
/// catcher's `MAIL_*`.
fn extra_env(
    cfg: &Config,
    site: Option<&Site>,
    tool: &str,
    args: &[String],
) -> Result<Vec<(String, String)>> {
    let Some(s) = site else {
        return Ok(Vec::new());
    };
    let test = is_test_run(tool, args);
    let mut env = crate::worktrees::checked_env(cfg, s, test)?;
    env.extend(apps::shared_env(s));
    // The mail catcher's, as the app's processes get it (`processes::env`).
    if !test {
        env.extend(crate::mail::env(cfg, s));
    }
    Ok(env)
}

fn resolve_for(
    tool: &str,
    cwd: &Path,
    cfg: &Config,
    fetch: bool,
    site: Option<&Site>,
) -> Result<Resolved> {
    let h = home();
    match tool {
        "php" | "composer" | "laravel" => {
            // From the config: a site that serves no PHP itself still has its pin.
            let installed = installed_php();
            let pinned = site
                .and_then(|s| cfg.sites.get(&s.name))
                .and_then(|s| s.php.clone());
            let pinned = match site {
                Some(s) => crate::project::php_version(pinned, &s.path, &installed, None),
                None => pinned,
            };
            let v = php_version(pinned, cfg, &installed)?;
            let php_dir = crate::php_dir(&v);
            let php = crate::php_bin(&v);
            // What PHP-FPM loads (OPcache, the version's extensions), so CLI and web agree,
            // also in a PHP this one starts (`phpext::scan_env`).
            let scan = crate::phpext::scan_env(cfg, &v, site)?;
            if tool == "php" {
                return Ok((php, vec![], scan.into_iter().collect()));
            }
            let mut env = vec![path_with(&php_dir)];
            env.extend(scan);
            let phar = if fetch {
                create::composer()?
            } else {
                h.join("bin/composer.phar")
            };
            if tool == "composer" {
                return Ok((php, vec![phar.display().to_string()], env));
            }
            let composer_home = h.join("composer");
            env.push(("COMPOSER_HOME".into(), composer_home.display().to_string()));
            let laravel = composer_home.join("vendor/bin/laravel");
            if fetch && !laravel.exists() {
                run(crate::platform::command(&php)
                    .arg(&phar)
                    .args(["global", "require", "laravel/installer"])
                    .envs(env.clone()))?;
            }
            Ok((laravel, vec![], env))
        }
        "node" | "npm" | "npx" => {
            let v = match site {
                Some(s) if apps::runtime(&s.kind) == Some("node") => s.runtime_version.clone(),
                Some(s) => apps::runtime_version("node", &s.path),
                None => apps::runtime_version("node", cwd),
            }
            .context("No Node installed in Anvil: run `anvil runtime install node 24`")?;
            // npm and npx are .cmd scripts on Windows.
            let file = match tool {
                "node" => crate::platform::exe("node"),
                _ if cfg!(windows) => format!("{tool}.cmd"),
                _ => tool.to_string(),
            };
            let program = runtimes::node_bin(&v).join(file);
            if !program.exists() {
                bail!("Node {v} isn't installed in Anvil: run `anvil runtime install node {v}`");
            }
            Ok((program, vec![], runtimes::env("node", &v)))
        }
        // The user's pnpm, else Anvil's (`runtimes::with_pms`), with the site's Node.js when
        // there is one; its env carries the app's "Shared dependencies" (`extra_env`).
        "pnpm" => {
            let mut env = match resolve_for("node", cwd, cfg, fetch, site) {
                Ok((_, _, env)) => env,
                Err(_) => vec![("PATH".into(), runtimes::with_pms(&crate::env_path()))],
            };
            // Never this shim again.
            let shims = h.join("shims");
            let (_, path) = env.iter_mut().find(|(k, _)| k == "PATH").unwrap();
            *path = std::env::join_paths(std::env::split_paths(&path).filter(|d| *d != shims))?
                .to_string_lossy()
                .into_owned();
            let path = path.clone();
            if fetch {
                runtimes::pnpm_ready(&env)?;
            }
            let program = runtimes::which("pnpm", &path)
                .context("No pnpm found: install it in Settings › Node.js")?;
            Ok((program, vec![], env))
        }
        _ => bail!(
            "Anvil has no shim for {tool}: use one of {}",
            TOOLS.join(", ")
        ),
    }
}

/// `~/.zshrc` always; `~/.bash_profile` only when it exists.
pub fn shell_rc_files(user_home: &Path) -> Vec<PathBuf> {
    let mut v = vec![user_home.join(".zshrc")];
    let bash = user_home.join(".bash_profile");
    if bash.exists() {
        v.push(bash);
    }
    v
}

/// The lines added to a shell rc file; `$HOME/.anvil` unless ANVIL_HOME points elsewhere.
pub fn path_block(anvil_home: &Path, user_home: &Path) -> String {
    let dir = if anvil_home == user_home.join(".anvil") {
        "$HOME/.anvil".to_string()
    } else {
        let mut s = String::new();
        for c in anvil_home.to_string_lossy().chars() {
            if matches!(c, '"' | '\\' | '$' | '`') {
                s.push('\\');
            }
            s.push(c);
        }
        s
    };
    format!("{BLOCK_START}\nexport PATH=\"{dir}/shims:$PATH\"\n{BLOCK_END}\n")
}

/// The file without Anvil's block; every other line as it was.
fn without_block(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let l = line.trim_end_matches('\n');
        if l == BLOCK_START {
            inside = true;
        } else if inside && l == BLOCK_END {
            inside = false;
        } else if !inside {
            out.push_str(line);
        }
    }
    out
}

pub fn has_path_block(file: &Path) -> bool {
    fs::read_to_string(file).is_ok_and(|t| t.lines().any(|l| l == BLOCK_START))
}

/// Add the block at the end of `file` (creating it); a block already there that
/// differs is replaced.
pub fn add_path_block(file: &Path, block: &str) -> Result<()> {
    let text = match fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("couldn't read {}", file.display())),
    };
    if text.contains(block) {
        return Ok(());
    }
    let mut new = without_block(&text);
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(block);
    fs::write(file, new).with_context(|| format!("couldn't write {}", file.display()))
}

pub fn remove_path_block(file: &Path) -> Result<()> {
    if !has_path_block(file) {
        return Ok(());
    }
    let text = fs::read_to_string(file)?;
    fs::write(file, without_block(&text))
        .with_context(|| format!("couldn't write {}", file.display()))
}

/// The user's own PATH (HKCU\\Environment), as Windows gives it to programs started later.
#[cfg(windows)]
fn user_path() -> Option<String> {
    let out = crate::platform::command("reg")
        .args(["query", "HKCU\\Environment", "/v", "Path"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    // `    Path    REG_EXPAND_SZ    C:\\a;C:\\b`
    text.lines()
        .find_map(|l| l.trim_start().strip_prefix("Path"))
        .and_then(|l| l.split_once("REG_"))
        .and_then(|(_, rest)| rest.split_once(char::is_whitespace))
        .map(|(_, v)| v.trim().to_string())
}

#[cfg(windows)]
fn on_user_path(dir: &Path) -> bool {
    user_path().is_some_and(|p| {
        p.split(';')
            .any(|d| Path::new(d.trim_end_matches('\\')).eq(dir))
    })
}

/// The shims are written and the user's PATH has them.
#[cfg(windows)]
pub fn ready() -> bool {
    home().join("shims").join("php.exe").exists() && on_user_path(&home().join("shims"))
}

/// Write the shims and put their folder first on the user's PATH; Windows tells running
/// programs, and terminals opened from now on have it.
#[cfg(windows)]
pub fn setup() -> Result<Vec<PathBuf>> {
    install(&home())?;
    let dir = home().join("shims");
    if !on_user_path(&dir) {
        // .NET's SetEnvironmentVariable keeps REG_EXPAND_SZ and announces the change.
        let script = "$d = $env:ANVIL_SHIMS; $p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
             if ($p) { $p = \"$d;$p\" } else { $p = $d }; [Environment]::SetEnvironmentVariable('Path', $p, 'User')";
        run(crate::platform::command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("ANVIL_SHIMS", &dir))?;
    }
    Ok(vec![])
}

/// The shims are written and some shell rc file puts this home's shims on PATH; a
/// block left by another ANVIL_HOME doesn't count, so setup rewrites it.
#[cfg(not(windows))]
pub fn ready() -> bool {
    let block = path_block(&home(), &user_home());
    home().join("shims/php").exists()
        && shell_rc_files(&user_home())
            .iter()
            .any(|f| has_this_block(f, &block))
}

#[cfg(any(not(windows), test))]
fn has_this_block(file: &Path, block: &str) -> bool {
    fs::read_to_string(file).is_ok_and(|t| t.contains(block))
}

/// Write the shims and put them on PATH in every shell rc file.
#[cfg(not(windows))]
pub fn setup() -> Result<Vec<PathBuf>> {
    install(&home())?;
    let user = user_home();
    let block = path_block(&home(), &user);
    let files = shell_rc_files(&user);
    for f in &files {
        add_path_block(f, &block)?;
    }
    Ok(files)
}

/// The shell rc files holding the PATH block.
pub fn rc_files_with_block() -> Vec<PathBuf> {
    shell_rc_files(&user_home())
        .into_iter()
        .filter(|f| has_path_block(f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("anvil-shims-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn path_block_is_added_once_and_removed_cleanly() {
        let d = tmp("rc");
        let rc = d.join(".zshrc");
        let original = "export FOO=1\n# Anvil end\nalias ll='ls -l'\n";
        fs::write(&rc, original).unwrap();
        let block = path_block(&d.join(".anvil"), &d);
        assert!(block.contains("export PATH=\"$HOME/.anvil/shims:$PATH\""));
        add_path_block(&rc, &block).unwrap();
        add_path_block(&rc, &block).unwrap();
        let text = fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches(BLOCK_START).count(), 1);
        assert!(text.starts_with(original));
        assert!(has_path_block(&rc));
        // Another home replaces the block instead of adding a second one.
        let other = path_block(Path::new("/opt/an\"vil"), &d);
        assert!(other.contains(r#"export PATH="/opt/an\"vil/shims:$PATH""#));
        add_path_block(&rc, &other).unwrap();
        let text = fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches(BLOCK_START).count(), 1);
        assert!(text.contains("/opt/an"));
        assert!(has_this_block(&rc, &other) && !has_this_block(&rc, &block));
        remove_path_block(&rc).unwrap();
        assert_eq!(fs::read_to_string(&rc).unwrap(), original);
        remove_path_block(&rc).unwrap();
        assert_eq!(fs::read_to_string(&rc).unwrap(), original);
        // A missing rc file is created.
        let new = d.join("new-rc");
        add_path_block(&new, &block).unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(), block);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn writes_executable_shims() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("install");
        install(&d).unwrap();
        for tool in TOOLS {
            let f = d.join("shims").join(tool);
            let text = fs::read_to_string(&f).unwrap();
            assert!(text.starts_with("#!/bin/sh\nexec '"), "{text}");
            assert!(text.ends_with(&format!("' shim {tool} \"$@\"\n")), "{text}");
            assert_eq!(
                fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn rc_files() {
        let d = tmp("files");
        assert_eq!(shell_rc_files(&d), vec![d.join(".zshrc")]);
        fs::write(d.join(".bash_profile"), "").unwrap();
        assert_eq!(
            shell_rc_files(&d),
            vec![d.join(".zshrc"), d.join(".bash_profile")]
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn cli_choice() {
        let u = Path::new("/Users/me");
        let none = |_: &Path| false;
        assert_eq!(
            pick_cli(Path::new("/x/target/debug/anvil"), u, none),
            PathBuf::from("/x/target/debug/anvil")
        );
        let app = Path::new("/Applications/Anvil.app/Contents/MacOS/anvil-app");
        let bundled = |p: &Path| p == Path::new("/Applications/Anvil.app/Contents/MacOS/anvil");
        assert_eq!(
            pick_cli(app, u, bundled),
            PathBuf::from("/Applications/Anvil.app/Contents/MacOS/anvil")
        );
        let cargo = |p: &Path| p == Path::new("/Users/me/.cargo/bin/anvil");
        assert_eq!(
            pick_cli(app, u, cargo),
            PathBuf::from("/Users/me/.cargo/bin/anvil")
        );
        assert_eq!(pick_cli(app, u, none), PathBuf::from("anvil"));
        assert_eq!(sh_quote("/a b/it's"), r"'/a b/it'\''s'");
    }

    #[test]
    fn innermost_site_and_its_php() {
        use crate::tests::site;
        let sites = [
            site("outer", "/p/outer", Some("8.3")),
            site("inner", "/p/outer/inner", None),
            site("other", "/p/outer-two", Some("8.2")),
        ];
        let f = |p: &str| site_for(&sites, Path::new(p)).map(|s| s.name.as_str());
        assert_eq!(f("/p/outer/app"), Some("outer"));
        assert_eq!(f("/p/outer/inner/src"), Some("inner"));
        assert_eq!(f("/p/outer-two"), Some("other"));
        assert_eq!(f("/p"), None);

        let cfg = Config {
            php: Some("8.4".into()),
            ..Config::default()
        };
        let installed = ["8.3".to_string(), "8.4".to_string()];
        assert_eq!(
            php_version(sites[0].php.clone(), &cfg, &installed).unwrap(),
            "8.3"
        );
        assert_eq!(
            php_version(sites[1].php.clone(), &cfg, &installed).unwrap(),
            "8.4"
        );
        assert_eq!(php_version(None, &cfg, &installed).unwrap(), "8.4");
        let err = php_version(sites[2].php.clone(), &cfg, &installed).unwrap_err();
        assert!(err.to_string().contains("anvil php install 8.2"));
        let none = Config::default();
        assert_eq!(php_version(None, &none, &installed).unwrap(), "8.4");
        let err = php_version(None, &none, &[]).unwrap_err();
        assert!(err.to_string().contains("No PHP installed in Anvil"));
    }

    #[test]
    fn shims_carry_the_worktree_env() {
        use crate::worktrees::tests::{git, tmp};
        // It reads ANVIL_HOME (installed_php, then resolve_with): never while another
        // test points it at a fake PHP of its own and then away again.
        let _env = crate::lock(&crate::ENV_LOCK);
        let root = tmp("shims");
        let shop = root.join("shop");
        fs::create_dir(&shop).unwrap();
        git(&shop, &["init", "-q", "-b", "main"]);
        // A Laravel app: its worktree gets the Laravel preset's keys.
        fs::write(shop.join("artisan"), "").unwrap();
        git(&shop, &["add", "artisan"]);
        git(&shop, &["commit", "-q", "-m", "i"]);
        fs::write(shop.join(".env"), "DB_DATABASE=shop\n").unwrap();
        let wt = shop.join(".claude/worktrees/feature-x");
        git(
            &shop,
            &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "fx"],
        );
        fs::create_dir_all(wt.join("app")).unwrap();
        let mut cfg = Config::default();
        cfg.paths.push(root);
        let has_db = |env: &[(String, String)]| {
            env.iter()
                .any(|(k, v)| k == "DB_DATABASE" && v == "shop_feature_x")
        };
        let all = sites(&cfg);
        let site = site_for(&all, &wt.join("app")).expect("the worktree's site");
        assert_eq!(site.name, "shop--feature-x");
        let extra = |s, args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            extra_env(&cfg, s, "php", &args).unwrap()
        };
        assert!(has_db(&extra(Some(site), &["artisan", "migrate"])));
        assert!(extra(site_for(&all, &shop), &[]).is_empty());
        // A test run gets the database its own test config names; the prefixes stay.
        assert!(
            has_db(&extra(Some(site), &["artisan", "test"])),
            "no test config yet"
        );
        fs::write(
            wt.join("phpunit.xml"),
            r#"<phpunit><php><env name="DB_DATABASE" value=":memory:"/></php></phpunit>"#,
        )
        .unwrap();
        let env = extra(Some(site), &["artisan", "test"]);
        assert!(
            env.iter()
                .any(|(k, v)| k == "DB_DATABASE" && v == ":memory:")
        );
        assert!(env.iter().any(|(k, _)| k == "REDIS_PREFIX"));
        assert!(has_db(&extra(Some(site), &["artisan", "migrate:fresh"])));
        // Through the real shim when this machine has an Anvil PHP (CI has none).
        if !installed_php().is_empty() {
            let (_, _, env) = resolve_with("php", &wt.join("app"), &cfg, false).unwrap();
            assert!(has_db(&env), "{env:?}");
        }
    }

    #[test]
    fn a_terminal_php_sends_to_the_mail_catcher_but_a_test_run_doesnt() {
        let site = crate::tests::site("shop", "/s/shop/public", Some("8.4"));
        let cfg = Config {
            mail_catcher: true,
            ..Config::default()
        };
        let user = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            extra_env(&cfg, Some(&site), "php", &args)
                .unwrap()
                .into_iter()
                .any(|(k, v)| k == "MAIL_USERNAME" && v == "shop")
        };
        assert!(user(&["artisan", "tinker"]));
        // phpunit.xml's MAIL_MAILER=array doesn't replace a set variable.
        assert!(!user(&["artisan", "test"]));
    }

    #[test]
    fn test_runs_are_told_apart() {
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            is_test_run("php", &args)
        };
        assert!(run(&["artisan", "test", "--parallel"]));
        assert!(run(&["-d", "memory_limit=1G", "artisan", "test"]));
        assert!(run(&["vendor/bin/phpunit"]));
        assert!(run(&["./vendor/bin/pest", "--filter", "x"]));
        assert!(run(&["/w/shop/vendor/bin/paratest"]));
        // Symfony's phpunit-bridge script.
        assert!(run(&["bin/phpunit"]));
        assert!(run(&["./bin/phpunit", "--filter", "x"]));
        assert!(run(&["vendor/bin/simple-phpunit"]));
        assert!(!run(&["bin/console"]));
        assert!(!run(&["artisan", "migrate"]));
        assert!(!run(&["artisan"]));
        assert!(!run(&["vendor/bin/phpstan"]));
        // Package managers' test scripts aren't told apart: they keep the worktree's own.
        assert!(!is_test_run("composer", &["test".into()]));
        assert!(!is_test_run("npm", &["test".into()]));
    }

    #[test]
    fn shims_give_a_test_run_its_test_database_and_everything_else_its_own() {
        use crate::worktrees::tests::{get, worktree_site};
        let (cfg, site) = worktree_site("shims-tests", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        let run = |tool: &str, args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            extra_env(&cfg, Some(&site), tool, &args).unwrap()
        };
        fs::write(site.path.join(".env.testing"), "DB_DATABASE=shop_testing\n").unwrap();
        assert_eq!(
            get(&run("php", &["artisan", "test"]), "DB_DATABASE"),
            Some("shop_testing")
        );
        for (tool, args) in [
            ("php", &["artisan", "migrate"][..]),
            ("composer", &["test"]),
            ("npm", &["run", "test:unit"]),
        ] {
            assert_eq!(
                get(&run(tool, args), "DB_DATABASE"),
                Some("shop_feature_x"),
                "{tool}"
            );
        }
        // A winning main's setting refuses the shim too (`worktrees::checked_env`).
        fs::write(
            site.path.join("phpunit.xml"),
            r#"<php><server name="DB_DATABASE" value="shop"/></php>"#,
        )
        .unwrap();
        let args = vec!["artisan".to_string(), "migrate".to_string()];
        assert!(extra_env(&cfg, Some(&site), "php", &args).is_err());
    }

    #[test]
    fn shims_refuse_a_worktree_without_its_own_database() {
        let (mut cfg, site) =
            crate::worktrees::tests::worktree_site("shims-refuse", "DATABASE_URL=redis://h/0\n");
        cfg.paths.push(site.path.parent().unwrap().to_path_buf());
        let err = resolve_with("php", &site.path, &cfg, false).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Anvil can't point main--x at its own database (DATABASE_URL uses a kind of database Anvil can't rewrite), so nothing of it runs"
        );
    }
}
