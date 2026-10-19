//! Terminal shims: `php`, `composer`, `laravel`, `node`, `npm` and `npx` in
//! `~/.anvil/shims` run `anvil shim <tool>`, which picks the version of the site
//! the current folder belongs to.

use crate::{Config, Site, apps, create, home, installed_php, run, runtimes, sites};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TOOLS: [&str; 6] = ["php", "composer", "laravel", "node", "npm", "npx"];

const BLOCK_START: &str = "# Anvil: php, composer, laravel and node follow each site's versions";
const BLOCK_END: &str = "# Anvil end";

/// `'…'` for sh, whatever the text holds.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The anvil CLI the shims run: this program when it is the CLI; from the app, the
/// CLI bundled next to it, else cargo's, else whatever `anvil` is on PATH.
pub fn pick_cli(exe: &Path, user_home: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if exe.file_name().is_some_and(|n| n == "anvil") {
        return exe.to_path_buf();
    }
    [
        exe.with_file_name("anvil"),
        user_home.join(".cargo/bin/anvil"),
    ]
    .into_iter()
    .find(|p| exists(p))
    .unwrap_or_else(|| "anvil".into())
}

use crate::user_home;

/// Write the shims into `<home>/shims`.
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
/// are resolved: macOS reaches /var through /private/var.
pub fn site_for<'a>(sites: &'a [Site], cwd: &Path) -> Option<&'a Site> {
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let cwd = real(cwd);
    sites
        .iter()
        .map(|s| (s, real(&s.path)))
        .filter(|(_, p)| cwd.starts_with(p))
        .max_by_key(|(_, p)| p.components().count())
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

/// `resolve` for a shim run with `args`. In a worktree, a test config that would put a
/// test run on main's database refuses every run (`check_test_config`); a test run
/// (`artisan test`, PHPUnit, Pest, ParaTest) whose test config names its own keeps it.
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
    let (extra, own_tests) = extra_env(cfg, site, tool, args)?;
    if fetch
        && !own_tests
        && let Some(s) = site.filter(|s| s.worktree_of.is_some())
    {
        // Its own database on first use, as a Start would; the line shows while it's made.
        // Refused when that fails: never run against a missing or half-made database.
        crate::worktrees::prepare(&mut crate::load()?, s, &mut |line| eprintln!("{line}"))?;
    }
    let mut r = resolve_for(tool, cwd, cfg, fetch, site)?;
    r.2.extend(extra);
    Ok(r)
}

/// `php artisan test`, `php vendor/bin/phpunit` (or pest, paratest; `./vendor/bin/pest`
/// reaches the shim the same way, through its `#!/usr/bin/env php`). Only to let a test
/// config's own database through: a test run this misses (`composer test`, a named
/// script) keeps the worktree's own database, never main's.
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
    ["phpunit", "pest", "paratest"]
        .iter()
        .any(|t| script.ends_with(Path::new("vendor/bin").join(t)))
}

const DB_KEYS: [&str; 3] = ["DB_DATABASE", "DB_URL", "DATABASE_URL"];

/// A database setting in the project's test config.
#[derive(Debug)]
struct TestVar {
    key: &'static str,
    value: String,
    /// It wins over a variable that is set already: `<env force="true">`, `<server>`.
    forced: bool,
    from: &'static str,
}

/// The database settings the project's test config names: `<env>` and `<server>` in
/// phpunit.xml(.dist) (not commented out), and `.env.testing`.
fn test_config(dir: &Path) -> Vec<TestVar> {
    let mut out = Vec::new();
    for from in ["phpunit.xml", "phpunit.xml.dist"] {
        let mut xml = fs::read_to_string(dir.join(from)).unwrap_or_default();
        while let Some(a) = xml.find("<!--") {
            let b = xml[a..].find("-->").map_or(xml.len(), |b| a + b + 3);
            xml.replace_range(a..b, "");
        }
        for part in xml.split('<').skip(1) {
            let server = part.starts_with("server");
            if !server && !part.starts_with("env") {
                continue;
            }
            let tag = &part[..part.find('>').unwrap_or(part.len())];
            let attr = |name: &str| {
                ['"', '\''].iter().find_map(|q| {
                    let at = tag.find(&format!(" {name}={q}"))? + name.len() + 3;
                    Some(tag[at..][..tag[at..].find(*q)?].to_string())
                })
            };
            let Some(key) = attr("name").and_then(|n| DB_KEYS.into_iter().find(|k| *k == n)) else {
                continue;
            };
            out.push(TestVar {
                key,
                value: attr("value").unwrap_or_default(),
                forced: server || attr("force").as_deref() == Some("true"),
                from,
            });
        }
    }
    let testing = crate::resources::dotenv_file(&dir.join(".env.testing"));
    for key in DB_KEYS {
        if let Some(v) = testing.get(key).filter(|v| !v.is_empty()) {
            out.push(TestVar {
                key,
                value: v.clone(),
                forced: false,
                from: ".env.testing",
            });
        }
    }
    out
}

/// Refuses a folder in a git worktree of an Anvil app (a hidden one too) that isn't
/// that worktree's site: it would run with no worktree env, on main's database.
fn known_worktree(cfg: &Config, site: Option<&Site>, cwd: &Path) -> Result<()> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let Some(root) = cwd.ancestors().find(|d| d.join(".git").exists()) else {
        return Ok(());
    };
    let Some(main) = crate::worktrees::main_of(root) else {
        return Ok(()); // not a linked worktree
    };
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
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

/// A worktree's own database and prefixes (`worktrees::env`), so a terminal `php artisan
/// migrate` stays off the main app's; `true` when a test run keeps its own test database
/// (one its test config names that isn't main's).
fn extra_env(
    cfg: &Config,
    site: Option<&Site>,
    tool: &str,
    args: &[String],
) -> Result<(Vec<(String, String)>, bool)> {
    use crate::worktrees::{is_mains_database, url_database, with_db};
    let Some(s) = site else {
        return Ok((Vec::new(), false));
    };
    let mut env = crate::worktrees::checked_env(cfg, s)?;
    if s.worktree_of.is_none() {
        return Ok((env, false));
    }
    let vars = test_config(&s.path);
    // Main's, or a URL Anvil can't read the database of.
    let mains = |v: &TestVar| match v.key {
        "DB_DATABASE" => is_mains_database(s, &v.value),
        _ => url_database(&v.value).is_none_or(|db| is_mains_database(s, &db)),
    };
    // Any run may be a test run (`composer test`, a named script): main's database in the
    // test config refuses them all when it would win over what Anvil sets.
    if let Some(v) = vars
        .iter()
        .find(|v| mains(v) && (v.forced || !env.iter().any(|(k, _)| k == v.key)))
    {
        bail!(
            "{} sets {} to {}'s database, so tests here would run on it: name a test database of its own there",
            v.from,
            v.key,
            s.worktree_of.as_deref().unwrap_or("main")
        );
    }
    let own_tests = is_test_run(tool, args) && !vars.is_empty() && !vars.iter().any(mains);
    if own_tests {
        // PHPUnit's <env> and .env.testing don't replace a variable that is set already: Anvil's
        // DB_DATABASE goes. Its URLs go too when the test config has its own, else they point
        // at the test config's database (a URL's database wins over DB_DATABASE).
        let main = crate::worktrees::main_of(&s.path).context("its main checkout wasn't found")?;
        env.retain(|(k, _)| k != "DB_DATABASE");
        let own_url = vars.iter().any(|v| v.key != "DB_DATABASE");
        let db = vars
            .iter()
            .find(|v| v.key == "DB_DATABASE")
            .map(|v| &v.value);
        for (k, v) in env.iter_mut() {
            if k != "DB_URL" && k != "DATABASE_URL" {
                continue;
            }
            match db.filter(|_| !own_url) {
                Some(db) => {
                    *v = with_db(v, db, &main, &s.path).with_context(|| {
                        format!("{k} uses a kind of database Anvil can't rewrite")
                    })?;
                }
                None => k.clear(),
            }
        }
        env.retain(|(k, _)| !k.is_empty());
    }
    Ok((env, own_tests))
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
            let php = php_dir.join("php");
            if tool == "php" {
                return Ok((php, vec![], vec![]));
            }
            let mut env = vec![path_with(&php_dir)];
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
                run(Command::new(&php)
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
            let program = runtimes::root("node").join(&v).join("bin").join(tool);
            if !program.exists() {
                bail!("Node {v} isn't installed in Anvil: run `anvil runtime install node {v}`");
            }
            Ok((program, vec![], runtimes::env("node", &v)))
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

/// The shims are written and some shell rc file puts them on PATH.
pub fn ready() -> bool {
    home().join("shims/php").exists()
        && shell_rc_files(&user_home())
            .iter()
            .any(|f| has_path_block(f))
}

/// Write the shims and put them on PATH in every shell rc file.
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
        let root = tmp("shims");
        let shop = root.join("shop");
        fs::create_dir(&shop).unwrap();
        git(&shop, &["init", "-q", "-b", "main"]);
        git(&shop, &["commit", "-q", "--allow-empty", "-m", "i"]);
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
        assert!(has_db(&extra(Some(site), &["artisan", "migrate"]).0));
        assert!(extra(site_for(&all, &shop), &[]).0.is_empty());
        // A test run keeps the database its own test config names; the prefixes stay.
        assert!(
            has_db(&extra(Some(site), &["artisan", "test"]).0),
            "no test config yet"
        );
        fs::write(
            wt.join("phpunit.xml"),
            r#"<phpunit><php><env name="DB_DATABASE" value=":memory:"/></php></phpunit>"#,
        )
        .unwrap();
        let (env, own) = extra(Some(site), &["artisan", "test"]);
        assert!(own && !has_db(&env));
        assert!(env.iter().any(|(k, _)| k == "REDIS_PREFIX"));
        assert!(has_db(&extra(Some(site), &["artisan", "migrate:fresh"]).0));
        // Through the real shim when this machine has an Anvil PHP (CI has none).
        if !installed_php().is_empty() {
            let (_, _, env) = resolve_with("php", &wt.join("app"), &cfg, false).unwrap();
            assert!(has_db(&env), "{env:?}");
        }
    }

    #[test]
    fn test_runs_and_their_own_test_database() {
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            is_test_run("php", &args)
        };
        assert!(run(&["artisan", "test", "--parallel"]));
        assert!(run(&["-d", "memory_limit=1G", "artisan", "test"]));
        assert!(run(&["vendor/bin/phpunit"]));
        assert!(run(&["./vendor/bin/pest", "--filter", "x"]));
        assert!(run(&["/w/shop/vendor/bin/paratest"]));
        assert!(!run(&["artisan", "migrate"]));
        assert!(!run(&["artisan"]));
        assert!(!run(&["vendor/bin/phpstan"]));
        // Package managers' test scripts aren't told apart: they keep the worktree's own.
        assert!(!is_test_run("composer", &["test".into()]));
        assert!(!is_test_run("npm", &["test".into()]));

        let d = crate::worktrees::tests::tmp("testdb");
        let vars = |d: &Path| -> Vec<(String, String, bool)> {
            test_config(d)
                .into_iter()
                .map(|v| (v.key.to_string(), v.value, v.forced))
                .collect()
        };
        assert!(vars(&d).is_empty());
        // Laravel's own phpunit.xml ships the line commented out.
        let xml = "<php>\n<!-- <env name=\"DB_DATABASE\" value=\":memory:\"/> -->\n<env name=\"APP_ENV\" value=\"testing\"/>\n</php>";
        fs::write(d.join("phpunit.xml"), xml).unwrap();
        assert!(vars(&d).is_empty());
        fs::write(
            d.join("phpunit.xml.dist"),
            "<env name='DB_DATABASE' value='t'/>",
        )
        .unwrap();
        assert_eq!(vars(&d), [("DB_DATABASE".into(), "t".into(), false)]);
        // `force` and `<server>` win over a variable that is set already.
        fs::write(
            d.join("phpunit.xml.dist"),
            r#"<env force="true" name="DB_DATABASE" value="shop_t"/><server name="DB_URL" value="mysql://h/x"/>"#,
        )
        .unwrap();
        assert_eq!(
            vars(&d),
            [
                ("DB_DATABASE".into(), "shop_t".into(), true),
                ("DB_URL".into(), "mysql://h/x".into(), true)
            ]
        );
        fs::remove_file(d.join("phpunit.xml.dist")).unwrap();
        fs::write(
            d.join(".env.testing"),
            "DB_DATABASE=shop_testing\nDATABASE_URL=postgres://h/t\n",
        )
        .unwrap();
        assert_eq!(
            vars(&d),
            [
                ("DB_DATABASE".into(), "shop_testing".into(), false),
                ("DATABASE_URL".into(), "postgres://h/t".into(), false)
            ]
        );
    }

    #[test]
    fn test_runs_never_use_mains_database() {
        use crate::worktrees::tests::worktree_site;
        let (cfg, site) = worktree_site("shims-tests", "DB_CONNECTION=pgsql\nDB_DATABASE=shop\n");
        let get = |env: &[(String, String)], key: &str| {
            env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        };
        let run = |tool: &str, args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            extra_env(&cfg, Some(&site), tool, &args)
        };
        let testing = |text: &str| fs::write(site.path.join(".env.testing"), text).unwrap();
        // Every run, tests too however they're started, gets the worktree's own database.
        for (tool, args) in [
            ("php", &["artisan", "test"][..]),
            ("composer", &["test"]),
            ("npm", &["run", "test:unit"]),
        ] {
            let (env, own) = run(tool, args).unwrap();
            assert!(!own);
            assert_eq!(
                get(&env, "DB_DATABASE").as_deref(),
                Some("shop_feature_x"),
                "{tool}"
            );
        }
        // A test config naming main's database doesn't win over Anvil's.
        testing("DB_DATABASE=shop\n");
        let (env, own) = run("php", &["artisan", "test"]).unwrap();
        assert!(!own);
        assert_eq!(get(&env, "DB_DATABASE").as_deref(), Some("shop_feature_x"));
        // One of its own does, for a test run only.
        testing("DB_DATABASE=shop_testing\n");
        let (env, own) = run("php", &["artisan", "test"]).unwrap();
        assert!(own && get(&env, "DB_DATABASE").is_none());
        let (env, _) = run("php", &["artisan", "migrate"]).unwrap();
        assert_eq!(get(&env, "DB_DATABASE").as_deref(), Some("shop_feature_x"));
        // A URL of main's (or one naming no database) that nothing of Anvil's overrides
        // would win: every run refuses.
        for url in [
            "DB_URL=pgsql://u@127.0.0.1:5433/shop\n",
            "DB_URL=redis://h\n",
        ] {
            testing(url);
            let e = run("composer", &["test"]).unwrap_err().to_string();
            assert_eq!(
                e,
                ".env.testing sets DB_URL to shop's database, so tests here would run on it: name a test database of its own there"
            );
        }
        fs::remove_file(site.path.join(".env.testing")).unwrap();
        // So does main's in a variable that wins over a set one.
        fs::write(
            site.path.join("phpunit.xml"),
            r#"<php><server name="DB_DATABASE" value="shop"/></php>"#,
        )
        .unwrap();
        let e = run("php", &["artisan", "migrate"]).unwrap_err().to_string();
        assert!(
            e.starts_with("phpunit.xml sets DB_DATABASE to shop's database"),
            "{e}"
        );
        fs::remove_file(site.path.join("phpunit.xml")).unwrap();

        // Its own DB_DATABASE while `.env` has a URL with main's: the URL is pointed at it.
        fs::write(
            site.path.join(".env"),
            "DB_CONNECTION=pgsql\nDB_URL=pgsql://u@127.0.0.1:5433/shop\n",
        )
        .unwrap();
        testing("DB_DATABASE=shop_testing\n");
        let (env, own) = run("php", &["artisan", "test"]).unwrap();
        assert!(own && get(&env, "DB_DATABASE").is_none());
        assert_eq!(
            get(&env, "DB_URL").as_deref(),
            Some("pgsql://u@127.0.0.1:5433/shop_testing")
        );
        // A URL of main's that Anvil's own DB_URL overrides is fine.
        testing("DB_URL=pgsql://u@127.0.0.1:5433/shop\n");
        let (env, own) = run("composer", &["test"]).unwrap();
        assert!(!own);
        assert_eq!(
            get(&env, "DB_URL").as_deref(),
            Some("pgsql://u@127.0.0.1:5433/shop_feature_x")
        );
    }

    #[test]
    fn a_test_sqlite_file_that_is_mains_isnt_the_tests_own() {
        use crate::worktrees::tests::worktree_site;
        // Main's SQLite path is relative: Anvil sets no DB_DATABASE, so `.env.testing` would win.
        let (cfg, site) = worktree_site("shims-sqlite", "DB_CONNECTION=sqlite\n");
        let main = crate::worktrees::main_of(&site.path).unwrap();
        let args = vec!["artisan".to_string(), "test".to_string()];
        let mains = main.join("database/database.sqlite");
        fs::create_dir_all(mains.parent().unwrap()).unwrap();
        fs::write(&mains, "").unwrap();
        let testing = format!("DB_DATABASE={}\n", mains.display());
        fs::write(site.path.join(".env.testing"), testing).unwrap();
        let e = extra_env(&cfg, Some(&site), "php", &args).unwrap_err();
        assert!(
            e.to_string()
                .starts_with(".env.testing sets DB_DATABASE to shop's database"),
            "{e}"
        );
        // One in the worktree is its own.
        let testing = format!("DB_DATABASE={}\n", site.path.join("t.sqlite").display());
        fs::write(site.path.join(".env.testing"), testing).unwrap();
        assert!(extra_env(&cfg, Some(&site), "php", &args).unwrap().1);
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
