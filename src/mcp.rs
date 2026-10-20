//! `anvil mcp`: Anvil's tools for AI agents, MCP (JSON-RPC 2.0) over stdio. Config
//! reads and writes run here, through `update`; process control goes to the running
//! app over the control socket. Nothing that can't be undone is a tool: no database
//! drop, no service data removal, no clean, no uninstall, no worktree remove --drop-db.

use crate::{Config, Site, apps, check, control, logs, worktrees};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// The newest MCP revision this speaks; an older one a client asks for is answered in kind.
const PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = [PROTOCOL, "2025-03-26", "2024-11-05"];

pub const NOT_RUNNING: &str = "Anvil isn't running. Open it, then try again.";
pub const TURNED_OFF: &str = "Turn on Agents (MCP) in Settings › Plugins";

/// Answers one JSON-RPC message per line until `input` ends. `initialize` sets `client`
/// to the agent's `clientInfo.name`, the name the app's activity list shows.
pub fn serve(input: impl BufRead, mut output: impl Write, client: &mut String) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let reply = json!({"jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": format!("Parse error: {e}")}});
                writeln!(output, "{reply}")?;
                output.flush()?;
                continue;
            }
        };
        let method = msg["method"].as_str().unwrap_or_default();
        let params = &msg["params"];
        let result = match method {
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    *client = name.into();
                }
                let asked = params["protocolVersion"].as_str().unwrap_or_default();
                let version = PROTOCOLS
                    .into_iter()
                    .find(|v| *v == asked)
                    .unwrap_or(PROTOCOL);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "anvil", "version": env!("CARGO_PKG_VERSION")},
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                if tools().iter().any(|t| t["name"] == name) {
                    let args = params.get("arguments").cloned().unwrap_or(json!({}));
                    Ok(match call(name, &args, client) {
                        Ok(v) => text(&v, false),
                        Err(e) => text(&Value::String(format!("{e:#}")), true),
                    })
                } else {
                    Err((-32602, format!("Unknown tool: {name}")))
                }
            }
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        // A notification (`notifications/initialized`, ...) gets no reply.
        let Some(id) = msg.get("id") else { continue };
        let reply = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        writeln!(output, "{reply}")?;
        output.flush()?;
    }
    Ok(())
}

/// A tool's answer: one text content, pretty JSON (or the error's words).
fn text(v: &Value, is_error: bool) -> Value {
    let t = match v {
        Value::String(s) => s.clone(),
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    json!({"content": [{"type": "text", "text": t}], "isError": is_error})
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": props, "required": required},
    })
}

fn tools() -> Vec<Value> {
    let site = json!({"type": "string", "description": "The app's name, as `apps` lists it"});
    let action = json!({"type": "string", "enum": ["start", "stop", "restart"]});
    vec![
        tool(
            "apps",
            "Every app and worktree Anvil serves: name, address, kind, the app a worktree belongs to, and its state",
            json!({}),
            &[],
        ),
        tool(
            "app",
            "One app: addresses, HTTPS, PHP and language version, its processes and their state, the services it uses (database, cache, mail), a worktree's own database, and whether Anvil's shims are on PATH",
            json!({"site": site}),
            &["site"],
        ),
        tool(
            "run",
            "Start, stop or restart one of an app's processes, or all of them, in the running Anvil",
            json!({"site": site, "process": {"type": "string", "description": "A process id from `app`; leave out for all"}, "action": action}),
            &["site", "action"],
        ),
        tool(
            "services",
            "Anvil's database, cache and mail services: version, port, state, the apps using them, and the .env lines to connect",
            json!({}),
            &[],
        ),
        tool(
            "service",
            "Start or stop one of Anvil's services",
            json!({"id": {"type": "string"}, "action": {"type": "string", "enum": ["start", "stop"]}}),
            &["id", "action"],
        ),
        tool(
            "logs",
            "An app's log entries, newest last: its own logs (Laravel, Symfony, PHP-FPM) and its processes' output",
            json!({
                "site": site,
                "source": {"type": "string", "description": "A log's id or name; leave out for the newest"},
                "level": {"type": "string", "enum": ["error", "warning", "info", "debug"]},
                "query": {"type": "string", "description": "Only entries containing this, any case"},
                "n": {"type": "integer", "minimum": 1, "maximum": 500, "default": 50},
            }),
            &["site"],
        ),
        tool(
            "check",
            "Did my change work? Since `cursor`: which processes exited (with their last lines), which logs restarted, and the new errors and warnings. Without a cursor: the state now and a cursor to start from. Pass the returned cursor next time",
            json!({"site": site, "cursor": {"type": "object", "description": "The `cursor` the last check returned"}}),
            &["site"],
        ),
        tool(
            "worktrees",
            "An app's git worktrees, each with its own address and database",
            json!({"site": site}),
            &["site"],
        ),
        tool(
            "worktree_add",
            "Add a worktree of an app next to it, on a branch (made from the app's current one when it doesn't exist). It gets its own address, processes and database. Returns the after_create commands; they are not run",
            json!({
                "site": site,
                "name": {"type": "string", "description": "The branch"},
                "copy_db": {"type": "boolean", "description": "Its database a copy of main's; leave out for the app's hooks' choice"},
            }),
            &["site", "name"],
        ),
        tool(
            "worktree_remove",
            "Remove a worktree's folder. Refuses with uncommitted changes or unpushed commits, and never drops its database",
            json!({"site": site, "name": {"type": "string", "description": "The worktree's name or branch"}}),
            &["site", "name"],
        ),
        tool(
            "settings",
            "An app's settings: kind, PHP and language version, HTTPS, php.ini overrides, start command, port, autostart",
            json!({"site": site}),
            &["site"],
        ),
        tool(
            "set",
            "Change one of an app's settings. php, node: a version, or empty for the default. https, autostart: true or false. ini: \"key=value\", or \"key=\" to clear. command: the start command, or empty for the default",
            json!({
                "site": site,
                "field": {"type": "string", "enum": ["php", "node", "https", "ini", "command", "autostart"]},
                "value": {"type": ["string", "boolean"]},
            }),
            &["site", "field", "value"],
        ),
    ]
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .with_context(|| format!("\"{key}\" is missing"))
}

fn find(cfg: &Config, name: &str) -> Result<Site> {
    let name = crate::site_name(name);
    crate::sites(cfg)
        .into_iter()
        .find(|s| s.name == name)
        .with_context(|| format!("No app named {name}"))
}

/// The running app's answer, or "Anvil isn't running".
fn ask(method: &str, params: Value, client: &str) -> Result<Value> {
    control::request(method, params, client)?.context(NOT_RUNNING)
}

/// A write made here, for the app's activity list; nothing when the app isn't running.
fn done(site: &str, what: String, client: &str) {
    control::notify_changed();
    let _ = control::request("set_done", json!({"site": site, "what": what}), client);
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn call(name: &str, args: &Value, client: &str) -> Result<Value> {
    let cfg = crate::load()?;
    match name {
        "apps" => {
            // One request for every app's state; none while Anvil is closed.
            let states = control::request("apps", json!({}), client).ok().flatten();
            let apps: Vec<Value> = crate::sites(&cfg)
                .iter()
                .map(|s| {
                    let mut a = json!({
                        "name": s.name,
                        "address": crate::site_url(&cfg, s),
                        "kind": s.kind,
                    });
                    if let Some(main) = &s.worktree_of {
                        a["worktree_of"] = json!(main);
                        a["branch"] = json!(s.branch);
                    }
                    if let Some(st) = states.as_ref().map(|v| &v["app_states"][&s.name])
                        && !st.is_null()
                    {
                        a["state"] = st.clone();
                    }
                    a
                })
                .collect();
            Ok(json!({"anvil_running": states.is_some(), "apps": apps}))
        }
        "app" => {
            let s = find(&cfg, arg(args, "site")?)?;
            let running = control::request("exits", json!({"site": s.name}), client)?;
            let processes = match &running {
                Some(rows) => rows.clone(),
                // Nothing runs without the app: what it would run, stopped.
                None => Value::Array(
                    crate::processes::detect(&s)
                        .into_iter()
                        .map(|p| json!({"key": crate::processes::key(&s.name, &p.id), "label": p.name, "state": "stopped", "port": p.port}))
                        .collect(),
                ),
            };
            let live = crate::services::Services::default().pids();
            let mut a = json!({
                "name": s.name,
                "address": crate::site_url(&cfg, &s),
                "aliases": s.aliases,
                "https": s.secure,
                "kind": s.kind,
                "path": s.path,
                "anvil_running": running.is_some(),
                "processes": processes,
                "services_used": crate::resources::services_used(&cfg, &s, &live),
                "shims_on_path": crate::shims::ready(),
            });
            if apps::is_php(&s.kind) {
                a["php"] = json!(s.php);
            }
            if let Some(rt) = apps::runtime(&s.kind) {
                a["runtime"] = json!({"id": rt, "version": s.runtime_version});
            }
            if let Some(main) = &s.worktree_of {
                a["worktree_of"] = json!(main);
                a["database"] = json!(worktree_db(&cfg, &s));
            }
            Ok(a)
        }
        "run" => {
            let mut params = json!({"site": arg(args, "site")?, "action": arg(args, "action")?});
            if let Some(p) = args["process"].as_str() {
                params["process"] = json!(p);
            }
            let overview = ask("run", params, client)?;
            let rows: Vec<Value> = overview["rows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| json!({"id": r["id"], "name": r["name"], "running": r["running"], "exited": r["crashed"]}))
                .collect();
            Ok(json!(rows))
        }
        "services" => {
            let list: Vec<Value> = crate::services::Services::default()
                .info(&cfg)
                .into_iter()
                .map(|s| {
                    json!({
                        "id": s.id, "kind": s.kind, "version": s.version, "port": s.port,
                        "installed": s.installed, "enabled": s.enabled,
                        // This process started none: the pid files say what runs.
                        "running": crate::services::runs_somewhere(&s.id),
                        "web": s.web, "apps": s.apps, "env": s.env,
                    })
                })
                .collect();
            Ok(json!(list))
        }
        "service" => {
            let (id, action) = (arg(args, "id")?, arg(args, "action")?);
            if !matches!(action, "start" | "stop") {
                bail!("Not an action: {action} (start or stop)");
            }
            let rows = ask("service", json!({"id": id, "action": action}), client)?;
            let row = rows
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["id"] == id)
                .cloned();
            Ok(row.unwrap_or(Value::Null))
        }
        "logs" => {
            let s = find(&cfg, arg(args, "site")?)?;
            let n = args["n"].as_u64().unwrap_or(50).clamp(1, 500) as usize;
            let src = match args["source"].as_str() {
                Some(id) => logs::find(&s, id)?,
                None => logs::sources(&s)
                    .into_iter()
                    .next()
                    .with_context(|| format!("{} has no logs yet", s.name))?,
            };
            let path = std::path::Path::new(&src.path);
            let (level, query) = (args["level"].as_str(), args["query"].as_str());
            let mut entries = Vec::new();
            if level.is_some() || query.is_some() {
                entries = logs::search(path, query.unwrap_or(""), level, None, n)?.entries;
            } else {
                let mut before = None;
                while entries.len() < n {
                    let page = logs::read(path, before, logs::PAGE)?;
                    entries.extend(page.entries);
                    if !page.truncated {
                        break;
                    }
                    before = Some(page.start_offset);
                }
                entries.truncate(n);
            }
            let entries: Vec<Value> = entries
                .into_iter()
                .rev()
                .map(|e| {
                    let mut v = json!({"time": e.time, "level": e.level, "message": e.message});
                    if !e.trace.is_empty() {
                        // ponytail: the first frames only; the Logs tab has the whole trace.
                        v["trace"] = json!(e.trace.iter().take(5).collect::<Vec<_>>());
                    }
                    v
                })
                .collect();
            Ok(json!({"source": src.id, "label": src.label, "entries": entries}))
        }
        "check" => {
            let s = find(&cfg, arg(args, "site")?)?;
            let since: Option<check::Cursor> = match args.get("cursor") {
                None | Some(Value::Null) => None,
                Some(c) => Some(serde_json::from_value(c.clone()).context("Not a cursor")?),
            };
            let rows = ask("exits", json!({"site": s.name}), client)?;
            let procs: Vec<check::ProcState> = rows
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| check::ProcState {
                    key: r["key"].as_str().unwrap_or_default().into(),
                    label: r["label"].as_str().unwrap_or_default().into(),
                    state: r["state"].as_str().unwrap_or_default().into(),
                    exit: r["exit"]
                        .as_array()
                        .and_then(|e| Some((e.first()?.as_i64()? as i32, e.get(1)?.as_u64()?))),
                })
                .collect();
            Ok(serde_json::to_value(check::check(
                &s,
                &procs,
                since.as_ref(),
                now(),
            ))?)
        }
        "worktrees" => {
            let app = find(&cfg, arg(args, "site")?)?;
            let list: Vec<Value> = crate::sites(&cfg)
                .iter()
                .filter(|s| s.worktree_of.as_deref() == Some(&app.name))
                .map(|s| {
                    json!({
                        "name": s.name, "branch": s.branch, "path": s.path,
                        "address": crate::site_url(&cfg, s),
                        "database": worktree_db(&cfg, s),
                    })
                })
                .collect();
            Ok(json!(list))
        }
        "worktree_add" => {
            let app = find(&cfg, arg(args, "site")?)?;
            let branch = arg(args, "name")?;
            // ponytail: a branch is "there" locally or on origin; other remotes need `git fetch` first.
            let there = |r: &str| {
                crate::git::run(&app.path, &["rev-parse", "--verify", "--quiet", r]).is_ok()
            };
            let new = !there(&format!("refs/heads/{branch}"))
                && !there(&format!("refs/remotes/origin/{branch}"));
            let (folder, choice) =
                worktrees::add(&cfg, &app, branch, new, args["copy_db"].as_bool())?;
            let (cfg, ()) = crate::update(|c| worktrees::set_db_at(c, &folder, choice))?;
            let site = worktrees::site_at(&cfg, &folder)
                .with_context(|| format!("Anvil doesn't see {} as a worktree", folder.display()))?;
            done(&site.name, format!("added worktree {}", site.name), client);
            Ok(json!({
                "name": site.name,
                "branch": branch,
                "path": folder,
                "address": crate::site_url(&cfg, &site),
                "database": worktree_db(&cfg, &site),
                "after_create": worktrees::after_create(&cfg, &site),
                "note": "after_create wasn't run: run those commands in the worktree's folder",
            }))
        }
        "worktree_remove" => {
            let app = find(&cfg, arg(args, "site")?)?;
            let name = arg(args, "name")?;
            let full = format!("{}--{}", app.name, crate::site_name(name));
            let s = crate::sites(&cfg)
                .into_iter()
                .find(|s| {
                    s.worktree_of.as_deref() == Some(&app.name)
                        && (s.name == name || s.name == full || s.branch.as_deref() == Some(name))
                })
                .with_context(|| format!("{} has no worktree named {name}", app.name))?;
            if apps::running_anywhere(&s.name) {
                bail!("Stop {} first (run with action stop)", s.name);
            }
            // Never --drop-db, never --force. Saved after Ok and Err alike, as the CLI does.
            let (_, result) = crate::update(|c| Ok(worktrees::remove(c, &s, false, false)))?;
            result?;
            done(&s.name, format!("removed worktree {}", s.name), client);
            Ok(json!({"removed": s.name, "database": "kept"}))
        }
        "settings" => settings(&cfg, &find(&cfg, arg(args, "site")?)?),
        "set" => {
            let s = find(&cfg, arg(args, "site")?)?;
            let field = arg(args, "field")?;
            let value = &args["value"];
            let word = match value {
                Value::Bool(b) => b.to_string(),
                Value::String(v) => v.trim().to_string(),
                _ => bail!("\"value\" is missing"),
            };
            let flag = || match word.as_str() {
                "true" | "on" => Ok(true),
                "false" | "off" => Ok(false),
                _ => bail!("{field} takes true or false"),
            };
            let unset = || (!word.is_empty() && word != "default").then(|| word.clone());
            let name = s.name.as_str();
            let what = match field {
                "php" => {
                    crate::update(|c| crate::set_site_php(c, name, unset()))?;
                    format!("set PHP to {}", unset().unwrap_or("the default".into()))
                }
                "node" => {
                    crate::update(|c| crate::set_site_runtime(c, name, unset()))?;
                    format!(
                        "set the version to {}",
                        unset().unwrap_or("automatic".into())
                    )
                }
                "https" => {
                    let on = flag()?;
                    if !apps::serves(&s.kind) {
                        bail!("{name} is an app Anvil doesn't serve, so it has no HTTPS");
                    }
                    crate::update(|c| crate::set_secure(c, name, on))?;
                    format!("turned HTTPS {}", if on { "on" } else { "off" })
                }
                "ini" => {
                    let (key, v) = word
                        .split_once('=')
                        .context("ini takes \"key=value\", or \"key=\" to clear")?;
                    let v = Some(v.trim()).filter(|v| !v.is_empty());
                    crate::update(|c| crate::set_site_ini(c, name, key, v))?;
                    match v {
                        Some(v) => format!("set {} to {v}", key.trim()),
                        None => format!("cleared {}", key.trim()),
                    }
                }
                "command" => {
                    let cmd = Some(word.clone()).filter(|w| !w.is_empty());
                    crate::update(|c| crate::set_site_app(c, name, None, Some(cmd), None, None))?;
                    "set the start command".into()
                }
                "autostart" => {
                    let on = flag()?;
                    crate::update(|c| crate::set_site_app(c, name, None, None, None, Some(on)))?;
                    format!("turned autostart {}", if on { "on" } else { "off" })
                }
                f => bail!("Not a field: {f} (php, node, https, ini, command or autostart)"),
            };
            done(name, what, client);
            let cfg = crate::load()?;
            settings(&cfg, &find(&cfg, name)?)
        }
        _ => unreachable!("tools/call checks the name"),
    }
}

fn settings(cfg: &Config, s: &Site) -> Result<Value> {
    let own = cfg.sites.get(&s.name).cloned().unwrap_or_default();
    Ok(json!({
        "name": s.name,
        "kind": s.kind,
        "kind_detected": s.kind_detected,
        "php": s.php,
        "php_pinned": s.php_pinned,
        "runtime_version": s.runtime_version,
        "runtime_pinned": s.runtime_pinned,
        "https": s.secure,
        "ini": s.ini,
        "command": s.command,
        "command_custom": s.command_custom,
        "port": s.port,
        "autostart": own.autostart,
    }))
}

/// A worktree's own database: a name on a server, or its SQLite file; `None` without one.
fn worktree_db(cfg: &Config, s: &Site) -> Option<String> {
    match worktrees::main_db(cfg, s) {
        worktrees::MainDb::None => None,
        worktrees::MainDb::Sqlite { to, .. } => Some(to.display().to_string()),
        _ => Some(worktrees::own_db(cfg, s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    /// ANVIL_HOME at a fresh short dir (the socket path's limit), under the env lock.
    struct Home(
        PathBuf,
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
    );
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home() -> Home {
        static N: AtomicU32 = AtomicU32::new(0);
        let guard = crate::lock(&crate::ENV_LOCK);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(format!("/tmp/anvil-mcp-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        // SAFETY: every test that sets ANVIL_HOME holds ENV_LOCK.
        unsafe { std::env::set_var("ANVIL_HOME", &dir) };
        Home(dir, guard)
    }

    /// An app `name` linked from `<home>/apps/<name>`, of `kind`.
    fn app(h: &Home, name: &str, kind: &str) -> PathBuf {
        let dir = h.0.join("apps").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        crate::update(|c| {
            c.links.insert(name.into(), dir.clone());
            c.sites.entry(name.into()).or_default().kind = Some(kind.into());
            Ok(())
        })
        .unwrap();
        dir
    }

    /// The JSON-RPC replies to `msgs`, one per request.
    fn rpc(msgs: &[Value]) -> Vec<Value> {
        let input: String = msgs.iter().map(|m| format!("{m}\n")).collect();
        let mut out = Vec::new();
        let mut client = "Agent".to_string();
        serve(input.as_bytes(), &mut out, &mut client).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn call_tool(name: &str, args: Value) -> (String, bool) {
        let r = rpc(&[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}})]);
        let res = &r[0]["result"];
        (
            res["content"][0]["text"].as_str().unwrap().to_string(),
            res["isError"] == true,
        )
    }

    /// A stand-in app on the socket: every method it was sent, `ok` for each.
    fn listener() -> Arc<Mutex<Vec<String>>> {
        let seen = Arc::new(Mutex::new(vec![]));
        let s = seen.clone();
        let l = control::bind().unwrap();
        std::thread::spawn(move || {
            control::serve(
                l,
                Arc::new(move |r| {
                    s.lock().unwrap().push(r.method.clone());
                    Ok(Value::Null)
                }),
            )
        });
        seen
    }

    #[test]
    fn initialize_reports_tools_capability() {
        let mut out = Vec::new();
        let mut client = "Agent".to_string();
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-03-26", "clientInfo": {"name": "Claude Code"}}});
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let input = format!("{init}\n{note}\n");
        serve(input.as_bytes(), &mut out, &mut client).unwrap();
        let out = String::from_utf8(out).unwrap();
        // The notification gets no reply.
        assert_eq!(out.lines().count(), 1, "{out}");
        let r: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "anvil");
        assert_eq!(client, "Claude Code");
    }

    #[test]
    fn tools_list_has_exactly_the_v1_tools() {
        let r = rpc(&[json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})]);
        let tools = r[0]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "apps",
                "app",
                "run",
                "services",
                "service",
                "logs",
                "check",
                "worktrees",
                "worktree_add",
                "worktree_remove",
                "settings",
                "set"
            ]
        );
        for t in tools {
            assert_eq!(t["inputSchema"]["type"], "object", "{t}");
            let name = t["name"].as_str().unwrap();
            for word in ["drop", "delete", "clean", "uninstall"] {
                assert!(!name.contains(word), "{name}");
            }
        }
    }

    #[test]
    fn tools_list_offers_nothing_irreversible() {
        let r = rpc(&[json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})]);
        let all = r[0]["result"]["tools"].to_string().to_lowercase();
        // No database drop, no service data removal, no clean, no uninstall, no --drop-db.
        for word in [
            "drop_db",
            "drop-db",
            "dropdb",
            "drop_database",
            "drop database",
            "remove_data",
            "delete",
            "clean",
            "uninstall",
            "force",
        ] {
            assert!(!all.contains(word), "{word} in tools/list");
        }
        let remove = r[0]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "worktree_remove")
            .unwrap();
        let props = remove["inputSchema"]["properties"].as_object().unwrap();
        let mut keys: Vec<_> = props.keys().collect();
        keys.sort();
        assert_eq!(keys, ["name", "site"]);
        assert!(
            remove["description"]
                .as_str()
                .unwrap()
                .contains("never drops")
        );
    }

    #[test]
    fn apps_lists_a_linked_site() {
        let h = home();
        app(&h, "shop", "folder");
        let (text, err) = call_tool("apps", json!({}));
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["anvil_running"], false);
        assert_eq!(v["apps"][0]["name"], "shop");
        assert_eq!(v["apps"][0]["address"], "http://shop.test");
    }

    #[test]
    fn logs_returns_entries() {
        let h = home();
        let dir = app(&h, "shop", "laravel");
        std::fs::create_dir_all(dir.join("storage/logs")).unwrap();
        std::fs::write(
            dir.join("storage/logs/laravel.log"),
            "[2026-10-20 14:02:01] local.INFO: booted\n[2026-10-20 14:02:03] local.ERROR: boom\n",
        )
        .unwrap();
        let (text, err) = call_tool("logs", json!({"site": "shop"}));
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["source"], "laravel.log");
        let msgs: Vec<&str> = v["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["message"].as_str().unwrap())
            .collect();
        assert_eq!(msgs, ["booted", "boom"]);
        let (text, _) = call_tool("logs", json!({"site": "shop", "level": "error"}));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn set_php_writes_config_and_notifies() {
        let h = home();
        app(&h, "shop", "laravel");
        std::fs::create_dir_all(h.0.join("php/8.4")).unwrap();
        std::fs::write(h.0.join("php/8.4/php-fpm"), "").unwrap();
        let seen = listener();
        let (text, err) = call_tool(
            "set",
            json!({"site": "shop", "field": "php", "value": "8.4"}),
        );
        assert!(!err, "{text}");
        assert_eq!(
            crate::load().unwrap().sites["shop"].php.as_deref(),
            Some("8.4")
        );
        assert_eq!(*seen.lock().unwrap(), ["config_changed", "set_done"]);
        let (text, err) = call_tool(
            "set",
            json!({"site": "shop", "field": "ini", "value": "memory_limit=1G"}),
        );
        assert!(!err, "{text}");
        assert_eq!(
            crate::load().unwrap().sites["shop"].ini["memory_limit"],
            "1G"
        );
        call_tool(
            "set",
            json!({"site": "shop", "field": "ini", "value": "memory_limit="}),
        );
        assert!(crate::load().unwrap().sites["shop"].ini.is_empty());
        let (text, err) = call_tool(
            "set",
            json!({"site": "shop", "field": "port", "value": "1"}),
        );
        assert!(err && text.contains("Not a field"), "{text}");
    }

    #[test]
    fn run_without_app_says_open_it() {
        let h = home();
        app(&h, "shop", "folder");
        for (tool, args) in [
            ("run", json!({"site": "shop", "action": "restart"})),
            ("service", json!({"id": "mysql", "action": "start"})),
            ("check", json!({"site": "shop"})),
        ] {
            let (text, err) = call_tool(tool, args);
            assert!(err, "{tool}");
            assert_eq!(text, NOT_RUNNING, "{tool}");
        }
    }

    #[test]
    fn worktree_add_returns_after_create_without_running() {
        let h = home();
        let dir = app(&h, "shop", "folder");
        let git = crate::worktrees::tests::git;
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "i"]);
        let marker = h.0.join("marker");
        crate::update(|c| {
            let hooks = worktrees::Hooks {
                after_create: Some(vec![format!("touch {}", marker.display())]),
                ..Default::default()
            };
            c.worktree_presets.insert("other".into(), hooks);
            Ok(())
        })
        .unwrap();
        let (text, err) = call_tool("worktree_add", json!({"site": "shop", "name": "feature-x"}));
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["name"], "shop--feature-x");
        assert_eq!(
            v["after_create"],
            json!([format!("touch {}", marker.display())])
        );
        assert!(Path::new(v["path"].as_str().unwrap()).is_dir());
        assert!(!marker.exists());
        let (text, _) = call_tool("worktrees", json!({"site": "shop"}));
        assert!(text.contains("shop--feature-x"), "{text}");
        let (text, err) = call_tool(
            "worktree_remove",
            json!({"site": "shop", "name": "feature-x"}),
        );
        assert!(!err, "{text}");
        assert!(!h.0.join("apps/shop--feature-x").exists());
    }

    #[test]
    fn unknown_method_is_32601() {
        let r = rpc(&[json!({"jsonrpc": "2.0", "id": 7, "method": "resources/list"})]);
        assert_eq!(r[0]["id"], 7);
        assert_eq!(r[0]["error"]["code"], -32601);
        let r = rpc(&[json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": {"name": "clean", "arguments": {}}})]);
        assert_eq!(r[0]["error"]["code"], -32602);
    }
}
