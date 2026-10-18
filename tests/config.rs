//! `anvil::update` under contention: threads of this process and `anvil park` from
//! other processes change config.toml at once, and no change is lost. Its own test
//! binary, because it sets ANVIL_HOME for the whole process.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn concurrent_updates_keep_every_change() {
    let root = std::env::temp_dir().join(format!("anvil-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let root = root.canonicalize().unwrap();
    // SAFETY: the only test in this binary, set before any thread starts.
    unsafe { std::env::set_var("ANVIL_HOME", &home) };

    let parked: Vec<PathBuf> = (0..6).map(|i| root.join(format!("cli-{i}"))).collect();
    for p in &parked {
        fs::create_dir_all(p).unwrap();
    }
    let cli: Vec<_> = parked
        .iter()
        .map(|p| {
            Command::new(env!("CARGO_BIN_EXE_anvil"))
                .arg("park")
                .arg(p)
                .env("ANVIL_HOME", &home)
                .spawn()
                .unwrap()
        })
        .collect();
    let threads: Vec<_> = (0..8)
        .map(|t| {
            std::thread::spawn(move || {
                for i in 0..10 {
                    anvil::update(|c| {
                        c.paths.push(PathBuf::from(format!("/thread-{t}-{i}")));
                        Ok(())
                    })
                    .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    for mut c in cli {
        assert!(c.wait().unwrap().success());
    }

    let cfg = anvil::load().unwrap();
    assert_eq!(cfg.paths.len(), 8 * 10 + parked.len(), "{:?}", cfg.paths);
    for p in &parked {
        assert!(cfg.paths.contains(p), "{} lost", p.display());
    }
    let leftovers: Vec<_> = fs::read_dir(&home)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    fs::remove_dir_all(root).unwrap();
}
