# App Processes and the Processes Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every long-running piece of an app (Laravel's queue/logs/vite, extra package.json servers, Docker Compose services) becomes its own row with ▶/■/Restart and a log, one-off scripts run as Commands, and a separate Processes window shows everything Anvil runs with PID, CPU, memory and a live log.

**Architecture:** Detection stays in `src/processes.rs` (new sources: Laravel `DevCommands`, package.json server scripts) plus a new `src/compose.rs`. `Apps` (`src/apps.rs`) gains crash records and a listing accessor. The Tauri app gets richer process commands, an `activity` snapshot and a key-based log tail, and a new `processes.html` window. UI is the existing classic-script vanilla JS.

**Tech Stack:** Rust (anvil core crate + Tauri 2 app), vanilla JS/HTML/CSS in `app/ui`, `docker compose` CLI, the app's own PHP for Laravel.

**Spec:** `docs/superpowers/specs/2026-10-17-processes-window-design.md`

## Global Constraints

- No new crates. Shell out (`ps`, `docker compose`, `php`) as the code already does.
- UI copy in English. Never mention Herd or Lerd. Green = running; selection highlight neutral; orange only as brand accent. No clutter.
- The UI never passes a file path to the backend for logs: it passes a key; the backend maps key → file.
- Anvil never kills a Docker container: only `docker compose stop|restart|up -d <service>`. Quitting Anvil or stopping Serving leaves containers running.
- Compose containers of the project are listed whoever started them; nothing else outside Anvil is listed.
- Laravel rows, composer `dev` rows and `.anvil.toml` rows run exactly as written (no port, no flags). Only package.json server rows get a port, `--port $PORT --strictPort --host 127.0.0.1` for vite/vitepress/astro, and an address.
- Main server row of a non-PHP app answers at `<app>.<tld>`; other server rows at `<row>.<app>.<tld>`, row name cut at the first `:` (`docs:dev` → `docs.vuefinder.<tld>`), honouring the site's HTTPS choice (`Site.secure`).
- Process ids (keys, file names) use only letters, digits, `-`, `_`; a display name like `docs:dev` gets the id `docs-dev`.
- Every task: `cargo fmt`, `cargo clippy --workspace --all-targets` with no warnings, `cargo test --workspace` green. English commit messages, no Co-Authored-By trailer.

## Review Focus

1. A Laravel app whose boot fails or hangs (broken `.env`, no `vendor/`) → detection falls back to composer `dev` within 5 s; the Overview never hangs. Test in Task 2.
2. Docker not installed or the daemon down → the app's Containers group shows one line ("Docker isn't running") and no buttons; no repeated error toasts. Test in Task 6.
3. A process that crashes right after Restart → it stays red with its exit status, no automatic restart loop. Test in Task 4.
4. Script names with `:` or other characters (`docs:dev`, `test:e2e:dev`) → valid ids and log files, distinct from each other. Test in Task 1.
5. Two apps' extra server rows hashing to the same port, or onto a site/service port → distinct ports, stable across calls. Test in Task 3.

---

### Task 1: Classify package.json scripts into servers and commands

**Files:**
- Modify: `src/processes.rs` (add `Process.id`, `Process.server`; new fns)
- Test: `src/processes.rs` `mod tests`

**Interfaces:**
- Produces:
  - `Process { id: String, name: String, command: String, source: &'static str, server: bool }` (`id` is the key/file-name part; `name` is what the UI shows; existing sources set `server: false`, `id` = `name`).
  - `pub fn process_id(name: &str) -> String` — every char outside `[A-Za-z0-9_-]` becomes `-`.
  - `pub fn is_server_script(name: &str, command: &str) -> bool`
  - `pub fn package_scripts(dir: &Path) -> (Vec<Process>, Vec<String>)` — (server rows in package.json order with `source: "package.json"`, `server: true`, `command: "npm run <name>"`; names of all other scripts, excluding npm lifecycle hooks `pre*`/`post*`/`prepare`/`prepack`/`postinstall`).

- [ ] **Step 1: Write the failing test `package_scripts_split_servers_from_commands`** using vuefinder's real scripts (copy from `~/Sites/vuefinder/package.json` into a temp dir): servers = `["dev", "preview", "docs:dev", "docs:serve", "docs:preview"]`, and `test:e2e`, `test:e2e:dev` are commands (they wrap other servers); commands contain `build`, `test:unit`, `lint`, `format`, `type-check` and not `prepare`, `prepack`. Assert `process_id("docs:dev") == "docs-dev"`, `process_id("test:e2e:dev") == "test-e2e-dev"`, and that ids are unique for the vuefinder set.
- [ ] **Step 2: Run** `cargo test --lib package_scripts_split` → FAIL (not defined).
- [ ] **Step 3: Implement.** Server when the name is `dev`, `serve`, `start`, `preview` or ends in `:dev`/`:serve`/`:preview`, or the command's first word (after `npx`/`pnpm exec`/`yarn`) is one of `vite`, `vitepress dev`, `nuxt dev`, `next dev`, `astro dev`, `storybook dev`; never when the command starts with `start-server-and-test` or `run-s`/`npm-run-all` whose last step is not a server. Keep `is_server_script` a plain match, no regex crate.
- [ ] **Step 4: Run** `cargo test --lib processes` → PASS (existing tests updated for the new fields).
- [ ] **Step 5: Commit** `git commit -am "Tell package.json dev servers from one-off scripts"`

### Task 2: Read Laravel 13's dev commands from Laravel itself

**Files:**
- Modify: `src/processes.rs`
- Test: `src/processes.rs` `mod tests`

**Interfaces:**
- Consumes: `Process`, `process_id` (Task 1).
- Produces: `pub fn laravel_dev_commands(dir: &Path, php: &Path) -> Option<Vec<(String, String)>>` — `(name, command)` pairs as Laravel registers them, `server` removed; `None` when the class doesn't exist, PHP fails, prints no JSON, or takes over 5 s.

- [ ] **Step 1: Write failing tests** with a fake `php` (a temp `#!/bin/sh` script) instead of real PHP:
  - `laravel_list_drops_server`: fake prints `[{"name":"server","command":"php artisan serve"},{"name":"queue","command":"php artisan queue:listen --tries=1 --timeout=0"},{"name":"logs","command":"php artisan pail --timeout=0"},{"name":"vite","command":"pnpm run dev"}]` → `Some([("queue",…),("logs",…),("vite","pnpm run dev")])`.
  - `laravel_list_falls_back`: fake exits 1 → `None`; fake `sleep 10` → `None` in under 6 s; fake prints `not json` → `None`.
  - Project without `vendor/laravel/framework/src/Illuminate/Foundation/DevCommands.php` → `None` without running PHP (fake writes a marker file; assert it's absent).
- [ ] **Step 2: Run** `cargo test --lib laravel_list` → FAIL.
- [ ] **Step 3: Implement.** Run `php -r <script>` with `current_dir(dir)`; the script requires `vendor/autoload.php`, boots `bootstrap/app.php`'s console kernel, and `echo json_encode(array_map(fn($c) => ['name' => $c['name'], 'command' => $c['command']], \Illuminate\Foundation\DevCommands::commands()));` (verified on ozdemir.be). Kill the child after 5 s (`try_wait` loop, as `terminate_group` does). Cache the parsed result in `home()/cache/devcommands-<hash of dir>.json` together with the mtimes of `composer.lock` and `app/Providers/`; reuse while they match.
- [ ] **Step 4: Wire into `detect`:** source order `.anvil.toml` → `laravel_dev_commands` (PHP apps only, `php` = `home()/php/<site.php>/php`, `source: "artisan dev"`) → composer `concurrently` → package.json. Existing test `project_file_comes_first_then_dev_scripts` keeps passing; add an assertion that a Laravel 13 fixture (DevCommands.php present + fake php on the site's PHP path via `ANVIL_HOME` in a temp dir) yields `["queue","logs","vite"]`.
- [ ] **Step 5: Run** `cargo test --workspace` → PASS.
- [ ] **Step 6: Commit** `git commit -am "Read Laravel 13's dev processes from the app"`

### Task 3: Ports, flags and addresses for extra server rows

**Files:**
- Modify: `src/apps.rs` (`assign_ports` helper), `src/processes.rs` (`detect` for non-PHP kinds), `src/lib.rs` (`caddyfile`, `Site`)
- Test: `src/lib.rs` and `src/processes.rs` tests, `tests/cli.rs` `caddyfile_for_a_few_sites`

**Interfaces:**
- Consumes: `package_scripts`, `Process.server` (Task 1).
- Produces:
  - `pub fn hashed_port(key: &str, base: u16, taken: &[u16], cfg: &Config) -> u16` in `src/apps.rs` (the existing djb2 loop, extracted; `assign_ports` uses it with base 41000; server rows use base 45000).
  - `pub struct ServerRow { pub id: String, pub host: String, pub port: u16 }` and `pub fn server_rows(cfg: &Config, site: &Site) -> Vec<ServerRow>` in `src/processes.rs`: every package.json server row of a non-PHP app except the one that is the app's own dev server (`dev`), host `<script name cut at the first ':'>.<site.host>` (`docs:dev` → `docs.vuefinder.<tld>`).
  - `Process.port: Option<u16>` filled for those rows; `detect` returns them for non-PHP kinds too.

- [ ] **Step 1: Write failing tests:**
  - `extra_servers_get_stable_distinct_ports`: two sites whose rows would collide get different ports; calling twice returns the same ports; never equal to `cfg.http_port`, `cfg.https_port`, a site port or a service port.
  - In `caddyfile_routes_php_sites_to_their_socket` (lib.rs): a vite-kind site `vuefinder` with a `docs:dev` server row → block `http://docs.vuefinder.test {\n\treverse_proxy 127.0.0.1:<port>` when `secure == false`, and `docs.vuefinder.test {` plus a redirect block when `secure == true`.
  - `default_command`-style assertion: the docs row's start command for a vitepress script ends with `-- --port $PORT --strictPort --host 127.0.0.1`.
- [ ] **Step 2: Run** `cargo test --workspace` → the new tests FAIL.
- [ ] **Step 3: Implement** (reuse the existing `addresses`/`redirect` closures in `caddyfile`; `Apps::start_process` sets `PORT` when `p.port` is `Some`).
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -am "Give an app's extra dev servers their own port and address"`

### Task 4: Crash records and a listing in `Apps`

**Files:**
- Modify: `src/apps.rs`
- Test: `src/apps.rs` `mod tests`

**Interfaces:**
- Produces:
  - `pub struct Exit { pub code: Option<i32>, pub at: u64 }` (`at` = unix seconds).
  - `Apps::running(&mut self, key)` now records an `Exit` in `self.exits` when a child ended without `stop`.
  - `pub fn crashed(&mut self) -> Vec<(String, Exit)>` (polls every child first).
  - `pub fn list(&mut self) -> Vec<(String, u32, bool)>` — running keys, process-group pid, `true` for app processes (`<site>@<id>`).
  - `stop(key)` and a successful `start`/`start_process` remove the key's `Exit`.
  - `pub fn run_once(&mut self, site: &Site, script: &str) -> Result<()>` — key `<site>#<process_id(script)>`, runs `npm run <script>` with `processes::env`, log `log_file(key)`; its normal exit is not a crash (exit code kept, shown as "finished" / "failed").

- [ ] **Step 1: Write failing tests** with a temp `ANVIL_HOME` and a fake site whose `.anvil.toml` defines `boom = "sh -c 'exit 3'"` and `idle = "sleep 30"`: after `start_process(boom)` and ~300 ms, `crashed()` holds `("<site>@boom", Exit{code: Some(3), ..})`; `list()` holds the idle key with its pid and `true`; `stop(idle)` leaves no crash record; a second `start_process(boom)` that crashes again leaves exactly one record (no restart loop, no duplicates).
- [ ] **Step 2: Run** `cargo test --lib apps` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** `git commit -am "Remember processes that exit on their own"`

### Task 5: Docker Compose projects

**Files:**
- Create: `src/compose.rs`
- Modify: `src/lib.rs` (`pub mod compose;`), `src/apps.rs` (`KINDS`, `label`, `detect`: kind `"compose"`, label `"Docker Compose"`, only when no other kind is detected), `src/lib.rs` `caddyfile` (no block for `compose`), `app/ui/app.js` kind list if it enumerates kinds
- Test: `src/compose.rs` `mod tests`, `src/apps.rs` detection test

**Interfaces:**
- Produces in `src/compose.rs`:
  - `pub fn file(dir: &Path) -> Option<PathBuf>` — first of `compose.yaml`, `compose.yml`, `docker-compose.yaml`, `docker-compose.yml`.
  - `#[derive(Serialize)] pub struct Container { pub service: String, pub state: String, pub health: Option<String>, pub ports: Vec<u16> }`
  - `pub fn containers(dir: &Path) -> Result<Vec<Container>>` — every service from `docker compose config --services`, merged with `docker compose ps --all --format json` (newline-delimited objects: `Service`, `State`, `Health`, `Publishers[].PublishedPort`); a service with no container has state `"stopped"`. Errors: docker missing → `"Docker isn't installed"`; daemon down → `"Docker isn't running"`. 5 s timeout per call.
  - `pub fn action(dir: &Path, service: &str, action: &str) -> Result<()>` — `start` → `up -d <svc>`, `stop` → `stop <svc>`, `restart` → `restart <svc>`; anything else is an error. Never `kill`/`down`.
  - `pub fn logs(dir: &Path, service: &str) -> Result<String>` — `docker compose logs --no-color --tail 200 <svc>`.
  - `pub(crate) fn parse_ps(json_lines: &str) -> Vec<(String, String, Option<String>, Vec<u16>)>`.

- [ ] **Step 1: Write failing tests:** `parse_ps` on a two-line fixture (`{"Service":"sphere","State":"running","Health":"healthy","Publishers":[{"PublishedPort":2593}]}` and `{"Service":"db","State":"exited","Health":"","Publishers":[]}`); `file()` finds `docker-compose.yml`; `apps::detect` of a dir with only `docker-compose.yml` + `README.md` is `"compose"`, of a Laravel dir with Sail's `docker-compose.yml` stays `"laravel"`; `action(dir, "x", "down")` is an error; `containers()` with `PATH` pointing at a fake `docker` that exits 1 with "Cannot connect to the Docker daemon" → `Err` whose text is `Docker isn't running`.
- [ ] **Step 2: Run** `cargo test --lib compose` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --workspace` → PASS. Then live, read-only: `cargo run -q -- sites | grep elpida` shows `Docker Compose`.
- [ ] **Step 5: Commit** `git commit -am "Detect Docker Compose projects and read their containers"`

### Task 6: Overview rows, design C

**Files:**
- Modify: `app/src/main.rs` (`ProcessRow`, `processes`, `process_run`; new `command_run`, `container_run`, `container_log`), `app/ui/processes.js`, `app/ui/style.css`, `app/ui/index.html` (the `.procs` box), `app/ui/logs.js` only if `showLog` needs a container source
- Test: headless smoke harness (below)

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces (Tauri):
  - `processes(site) -> Overview { rows: Vec<ProcessRow>, commands: Vec<String>, containers: Option<Vec<Container>>, docker_error: Option<String> }`
  - `ProcessRow` adds `id`, `server`, `address: Option<String>` (full URL via `site_url` logic), `pid: Option<u32>`, `crashed: Option<Exit>`.
  - `process_run(site, name: Option<String>, action)` keeps its contract (`name` is now the row `id`); `None` = Start all / Stop all.
  - `command_run(site, script) -> Overview`; `container_run(site, service, action) -> Overview`; `container_log(site, service) -> String`.

- [ ] **Step 1: Build the rows** (mockup C in `.superpowers/brainstorm/*/content/process-choice.html`): dot (green running, red crashed, grey stopped), name, address or `:port`, newest log line (muted, one line, ellipsis), right side Restart + ■ when running, ▶ when stopped, Restart on crashed rows; header buttons Start all / Stop all; a "Commands" line of chips; a "Containers" group for compose services with the same buttons, a single muted line instead when `docker_error` is set. Clicking a row opens its log (`showLog(site, "proc:<id>")`; containers use `container_log` text). Poll every 2 s while visible, redraw only on change (as today).
- [ ] **Step 2: Smoke test** — reuse the mocked-Tauri harness pattern (serve `app/ui` with a `mock.js` that defines `window.__TAURI__` before `app.js`; drive with puppeteer-core against `/Applications/Google Chrome.app`; mock globals must not clash with app.js names like `snap`). Mock `processes` to return vuefinder-like rows (`dev` running, `docs:dev` stopped, a crashed `queue`), commands `["build","lint"]`, and `docker_error: "Docker isn't running"`. Assert: 3 rows; the crashed row has Restart; clicking ▶ on `docs:dev` calls `process_run` with `{name:"docs-dev", action:"start"}`; the chip `build` calls `command_run`; the Containers group shows the docker message and no buttons; zero window errors.
- [ ] **Step 3:** `cargo clippy` (app crate) clean; `cargo test --workspace` green.
- [ ] **Step 4: Commit** `git commit -am "Show each running piece of an app as its own row"`

### Task 7: Activity backend for the Processes window

**Files:**
- Modify: `src/lib.rs` (`Server::pids() -> Vec<(String, u32)>`: `("Caddy", pid)` and `("PHP <v> FPM", pid)`), `src/resources.rs` (`pub struct Ps`, `pub fn ps()`), `app/src/main.rs` (new `activity`, `activity_log`)
- Test: `src/resources.rs` test for a usage helper; `app` compiles

**Interfaces:**
- Produces:
  - `pub fn usage(table: &[Ps], pid: u32, group: bool) -> (f32, u64)` in `src/resources.rs` — CPU % and RSS bytes summed over the process group (`group`) or the pid and its children.
  - Tauri `activity() -> Activity { web: Vec<Row>, apps: Vec<AppGroup>, services: Vec<Row> }`, `Row { key, name, pid: Option<u32>, port: Option<u16>, cpu: f32, memory: u64, state: "running"|"starting"|"crashed", detail: Option<String>, actions: bool }`, `AppGroup { site, rows: Vec<Row> }` — apps only when they have a running or crashed row; one `ps()` call per `activity()`.
  - Tauri `activity_log(key: String, from: u64) -> logs::Page` — key forms: `web:caddy`, `web:php-<v>`, `<site>`, `<site>@<id>`, `<site>#<id>`, `service:<id>`, `container:<site>/<service>`; anything else is an error; never a path from the UI. Containers return the `compose::logs` text as one page.
- [ ] **Step 1: Write the failing test** `usage_sums_a_group`: a hand-built `Ps` table (pgid 10 with two members 30%/100 MB and 5%/20 MB, an unrelated pid) → `(35.0, 120 MB)`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** `cargo test --workspace` → PASS; add a unit test that `activity_log` key parsing rejects `../etc/passwd` and `/tmp/x`.
- [ ] **Step 5: Commit** `git commit -am "List everything Anvil runs, with usage and logs"`

### Task 8: The Processes window

**Files:**
- Create: `app/ui/processes.html`, `app/ui/activity.js`
- Modify: `app/src/main.rs` (`open_processes` modelled on `open_settings`: label `processes`, title "Processes", 820×560, min 640×420, overlay title bar; `show_processes` command), `app/capabilities/default.json` (`"windows": ["main", "settings", "processes"]`), `app/ui/index.html` (icon button `#open-processes` between `#open-settings` and `#power`, same `icon-btn` class, list glyph, `title="Processes"`), `app/ui/app.js` (its click → `invoke("show_processes")`), `app/ui/style.css`
- Test: headless smoke harness

**Interfaces:**
- Consumes: `activity`, `activity_log`, `process_run`, `app_run`, `service_run`, `container_run` (Tasks 6–7).

- [ ] **Step 1: Build the window:** groups Web server / Applications (header per app, rows indented) / Services; columns name, PID, port, CPU, memory, status; Restart and Stop on rows with `actions`; crashed rows red with exit status, Stop dismisses; the selected row's log at the bottom, terminal order, following new lines via `activity_log(key, from)` every 2 s while visible; nothing running → "Nothing is running". `window.refresh` defined so `changed()` updates it.
- [ ] **Step 2: Smoke test** with a mocked `activity` (one Caddy row, one app with a running and a crashed row, one service): rows render in group order, Web server rows have no buttons, Stop on the crashed row calls `process_run` with `action:"stop"`, selecting a row calls `activity_log` with its key, zero window errors. Also load `index.html` and assert `#open-processes` sits between `#open-settings` and `#power`.
- [ ] **Step 3:** `cd app && cargo tauri build` succeeds.
- [ ] **Step 4: Commit** `git commit -am "Add the Processes window"`

### Task 9: Changelog and live check

**Files:**
- Modify: `CHANGELOG.md` (Unreleased › Added: per-app rows, Commands, Laravel 13 dev processes, Docker Compose projects, Processes window)

- [ ] **Step 1:** Write the entries (plain user-facing sentences, like the existing ones).
- [ ] **Step 2: Live read-only check** with the built app on this Mac: vuefinder shows `dev` and `docs:dev` rows plus Commands; ozdemir.be shows queue/logs/vite; elpida shows Docker Compose with four containers; the Processes window lists Caddy and PHP-FPM. Do not start or stop elpida's containers without asking the user (its world save takes up to 5 minutes).
- [ ] **Step 3: Commit** `git commit -am "Note the processes work in the changelog"`
