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

fn param<'a>(req: &'a Request, key: &str) -> Option<&'a str> {
    req.params.get(key).and_then(Value::as_str)
}

fn need(req: &Request, key: &str) -> Res<String> {
    param(req, key)
        .map(String::from)
        .ok_or_else(|| format!("{} needs \"{key}\"", req.method))
}

/// The site and what a request does, as the activity list words it; `None` for reads.
pub(crate) fn describe(req: &Request) -> Option<(Option<String>, String)> {
    let done = match param(req, "action")? {
        "start" => "started",
        "stop" => "stopped",
        "restart" => "restarted",
        other => other,
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

/// Answers one socket request; `run` and `service` land in the activity list, failed or not.
pub(crate) fn dispatch(handle: &AppHandle, req: &Request) -> Res<Value> {
    let out = answer(handle, req);
    if let Some((site, what)) = describe(req) {
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
            describe(&req("run", run)),
            Some((Some("shop".into()), "restarted api".into()))
        );
        let all = json!({"site": "shop", "action": "restart"});
        assert_eq!(
            describe(&req("run", all)),
            Some((Some("shop".into()), "restarted all processes".into()))
        );
        let svc = json!({"id": "mysql", "action": "stop"});
        assert_eq!(
            describe(&req("service", svc)),
            Some((None, "stopped mysql".into()))
        );
        assert_eq!(describe(&req("apps", json!({}))), None);
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
}
