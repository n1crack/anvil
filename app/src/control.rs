//! The control socket's methods: the CLI and agents reach the same functions as the
//! UI's buttons, so there is no second implementation.

use super::*;
use anvil::control::Request;
use serde_json::{Value, json};
use std::collections::VecDeque;

/// How many agent actions the app keeps in memory.
const LOG_CAP: usize = 200;

/// One action a client took through the socket: "Claude Code restarted api".
#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct AgentAction {
    /// Unix seconds.
    pub at: u64,
    pub client: String,
    pub site: Option<String>,
    pub what: String,
    pub ok: bool,
    pub detail: Option<String>,
}

pub(crate) fn record(log: &Mutex<VecDeque<AgentAction>>, a: AgentAction) {
    let mut log = anvil::lock(log);
    if log.len() >= LOG_CAP {
        log.pop_front();
    }
    log.push_back(a);
}

/// The actions newest first, for the UI; only one app's with `site`.
pub(crate) fn newest(log: &Mutex<VecDeque<AgentAction>>, site: Option<&str>) -> Vec<AgentAction> {
    let log = anvil::lock(log);
    log.iter()
        .rev()
        .filter(|a| site.is_none() || a.site.as_deref() == site)
        .cloned()
        .collect()
}

fn param<'a>(req: &'a Request, key: &str) -> Option<&'a str> {
    req.params.get(key).and_then(Value::as_str)
}

fn need(req: &Request, key: &str) -> Res<String> {
    param(req, key)
        .map(String::from)
        .ok_or_else(|| format!("{} needs \"{key}\"", req.method))
}

/// The site and what a request did, as the activity list words it ("restarted api"), or
/// what it failed to do ("couldn't restart api"); `None` for reads.
pub(crate) fn describe(req: &Request, ok: bool) -> Option<(Option<String>, String)> {
    // A config write `anvil mcp` made itself, told so the list shows it.
    if req.method == "set_done" {
        let what = need(req, "what").ok()?;
        let what = if ok { what } else { format!("couldn't {what}") };
        return Some((param(req, "site").map(String::from), what));
    }
    let done = match (ok, param(req, "action")?) {
        (false, a) => format!("couldn't {a}"),
        (true, "start") => "started".into(),
        (true, "stop") => "stopped".into(),
        (true, "restart") => "restarted".into(),
        (true, a) => a.into(),
    };
    match req.method.as_str() {
        "run" => Some((
            param(req, "site").map(String::from),
            format!(
                "{done} {}",
                param(req, "process").unwrap_or("all processes")
            ),
        )),
        "service" => Some((None, format!("{done} {}", param(req, "id")?))),
        _ => None,
    }
}

fn action(req: &Request) -> Res<String> {
    let a = need(req, "action")?;
    match a.as_str() {
        "start" | "stop" | "restart" => Ok(a),
        _ => Err(format!("Not an action: {a} (start, stop or restart)")),
    }
}

fn value(v: impl Serialize) -> Res<Value> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

/// Methods behind Settings › Features › Agents (MCP).
const MCP_ONLY: [&str; 2] = ["set_done", "exits"];

/// The app's processes for `check`: each with its log key, state and how it ended. A
/// process a signal ended (`Exit.code` None) reads as code -1.
fn exits(app: &App, site: &str) -> Res<Vec<Value>> {
    let (cfg, s) = find_site_cfg(site)?;
    Ok(process_rows(app, &cfg, &s)
        .into_iter()
        // A PHP or static app's row is the web server's, no process of its own.
        .filter(|r| r.kind != "site")
        .map(|r| {
            let key = if r.main {
                s.name.clone()
            } else {
                anvil::processes::key(&s.name, &r.process.id)
            };
            let state = match (&r.crashed, r.running) {
                (_, true) => "running",
                (Some(_), false) => "exited",
                (None, false) => "stopped",
            };
            let exit = r.crashed.as_ref().map(|e| (e.code.unwrap_or(-1), e.at));
            json!({"key": key, "label": r.process.name, "state": state,
                "port": r.process.port, "exit": exit})
        })
        .collect())
}

/// Answers one socket request; `run` and `service` land in the activity list, failed or not.
pub(crate) fn dispatch(handle: &AppHandle, req: &Request) -> Res<Value> {
    // Only `anvil mcp` sends these: refused, and not listed, while Agents (MCP) is off.
    if MCP_ONLY.contains(&req.method.as_str()) && !anvil::load().is_ok_and(|c| c.mcp) {
        return Err(anvil::mcp::TURNED_OFF.into());
    }
    let out = answer(handle, req);
    if let Some((site, what)) = describe(req, out.is_ok()) {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let a = AgentAction {
            at,
            client: req.client.clone(),
            site,
            what,
            ok: out.is_ok(),
            detail: out.as_ref().err().cloned(),
        };
        record(&handle.state::<App>().agent_log, a);
    }
    out
}

fn answer(handle: &AppHandle, req: &Request) -> Res<Value> {
    match req.method.as_str() {
        "ping" => Ok(json!({"version": env!("CARGO_PKG_VERSION")})),
        // The CLI wrote config.toml: what `refresh` does, then a re-render.
        "config_changed" => {
            reload(&handle.state::<App>())?;
            changed(handle);
            Ok(Value::Null)
        }
        "apps" => value(snapshot(&handle.state::<App>())?),
        "processes" => value(processes(handle.state(), need(req, "site")?)?),
        "run" => {
            let (site, action) = (need(req, "site")?, action(req)?);
            let name = param(req, "process").map(String::from);
            value(process_run(
                handle.clone(),
                handle.state(),
                site,
                name,
                action,
                None,
                None,
            )?)
        }
        "service" => {
            let (id, action) = (need(req, "id")?, action(req)?);
            value(service_run(handle.clone(), handle.state(), id, action)?)
        }
        "exits" => value(exits(&handle.state::<App>(), &need(req, "site")?)?),
        // `dispatch` records it; the config change came as `config_changed` before it.
        "set_done" => Ok(Value::Null),
        "activity" => {
            let site = param(req, "site");
            let app = handle.state::<App>();
            let log = anvil::lock(&app.agent_log);
            let wanted: Vec<_> = log
                .iter()
                .filter(|a| site.is_none() || a.site.as_deref() == site)
                .collect();
            value(wanted)
        }
        m => Err(format!("Not a method: {m}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(method: &str, params: Value) -> Request {
        Request {
            id: 1,
            method: method.into(),
            params,
            client: "t".into(),
        }
    }

    #[test]
    fn describe_names_the_action() {
        let run = json!({"site": "shop", "process": "api", "action": "restart"});
        assert_eq!(
            describe(&req("run", run.clone()), true),
            Some((Some("shop".into()), "restarted api".into()))
        );
        // A failure says what it couldn't do, not that it was done.
        assert_eq!(
            describe(&req("run", run), false),
            Some((Some("shop".into()), "couldn't restart api".into()))
        );
        let all = json!({"site": "shop", "action": "restart"});
        assert_eq!(
            describe(&req("run", all), true),
            Some((Some("shop".into()), "restarted all processes".into()))
        );
        let svc = json!({"id": "mysql", "action": "stop"});
        assert_eq!(
            describe(&req("service", svc), true),
            Some((None, "stopped mysql".into()))
        );
        assert_eq!(describe(&req("apps", json!({})), true), None);
        let set = json!({"site": "shop", "what": "set PHP to 8.4"});
        assert_eq!(
            describe(&req("set_done", set), true),
            Some((Some("shop".into()), "set PHP to 8.4".into()))
        );
    }

    #[test]
    fn log_is_capped() {
        let log = Mutex::new(VecDeque::new());
        for i in 0..205 {
            let what = format!("action {i}");
            let a = AgentAction {
                at: i,
                client: "t".into(),
                site: None,
                what,
                ok: true,
                detail: None,
            };
            record(&log, a);
        }
        let log = log.into_inner().unwrap();
        assert_eq!(log.len(), 200);
        assert_eq!(log.front().unwrap().what, "action 5");
        assert_eq!(log.back().unwrap().what, "action 204");
    }

    #[test]
    fn newest_first_and_by_site() {
        let log = Mutex::new(VecDeque::new());
        for (at, site) in [(1, Some("shop")), (2, None), (3, Some("shop"))] {
            let a = AgentAction {
                at,
                client: "t".into(),
                site: site.map(String::from),
                what: "w".into(),
                ok: true,
                detail: None,
            };
            record(&log, a);
        }
        let at = |v: Vec<AgentAction>| v.iter().map(|a| a.at).collect::<Vec<_>>();
        assert_eq!(at(newest(&log, None)), [3, 2, 1]);
        assert_eq!(at(newest(&log, Some("shop"))), [3, 1]);
    }
}
