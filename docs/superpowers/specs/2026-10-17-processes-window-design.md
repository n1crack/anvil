# Running things: app processes and the Processes window

## Intent

An app is often more than one running thing: Laravel has a queue, logs (Pail),
Vite, maybe Horizon; vuefinder has its test site (`dev`) and its docs
(`docs:dev`); elpida is a Docker Compose project with four services. A single
Start/Stop doesn't fit them. The user wants to see what runs, start and stop each
piece easily, read its log, and get a task-manager view of everything Anvil runs
("is Horizon running, what's in its log, did the job go out, what is active").
Choosing what runs has to feel good; a confusing picker makes people leave.

## Part 1: what runs, per app (the Overview)

The user picked design **C, every row on its own** (mockup in
`.superpowers/brainstorm/`, not committed):

- The Overview lists each long-running thing of the app as a row: status dot,
  name, address or port when it has one, the newest log line, and on the right
  **Restart** and **■ Stop** when running, **▶** when stopped. Clicking the row
  opens its full log.
- Above the rows, **Start all** / **Stop all**.
- No checkbox or "include in Start" selection step; each row is turned on and off
  directly.
- Below, **Commands**: one-off scripts as chips (`build`, `test:unit`, `lint`…).
  Clicking one runs it once; its output streams in the log; it ends by itself.

### Where rows come from (in this order; the first source that applies wins)

1. `.anvil.toml` `[processes]` (exists today), for any kind.
2. **Laravel 13+** (`Illuminate\Foundation\DevCommands` exists): ask Laravel for
   the list `php artisan dev` would run. Verified on ozdemir.be by booting the
   console kernel and calling `DevCommands::commands()`: server, queue, logs
   (Pail), vite, with their exact commands. Vendor packages that register dev
   commands (Horizon, Reverb…) come with it. `server` is dropped: Anvil serves
   the app through PHP-FPM. The PHP run uses the app's PHP version; it is cached
   by `composer.lock` + app service provider mtimes; a failure falls back to 3.
3. Older Laravel / PHP apps: `composer.json` `scripts.dev` split from
   `concurrently` (exists today, drops `server`).
4. `package.json` scripts: long-running servers become rows, the rest become
   Commands. A script is a server when its name is `dev`, `serve`, `start`,
   `preview`, or ends in `:dev` / `:serve`, or its command runs a known dev
   server (vite, vitepress, nuxt, next dev, astro dev, storybook). For Node-kind
   apps the `dev` row *is* the app's dev server (today's Start).
5. **Docker Compose** (`docker-compose.yml` / `compose.yaml` in the project).
   A project with no framework detected (elpida) becomes the new kind "Docker
   Compose" (no longer Static) and gets no address or Caddy block. A framework app
   that also ships a compose file (Laravel Sail) keeps its kind and serving; its
   compose services show as an extra **Containers** group under its rows. Unlike
   1–4, this source is added, not "first wins". Each compose service is a row. ▶ = `docker compose up -d <service>`, ■ =
   `docker compose stop <service>`, Restart = `docker compose restart <service>`,
   log = `docker compose logs -f --tail 200 <service>`, state from
   `docker compose ps --format json`. Docker not installed or not running → the
   rows show that, with no buttons.
   - **Lifetime (user's decision):** containers belong to Docker. Quitting Anvil
     or stopping Serving leaves them running; only ■ / Stop all stop them, through
     `docker compose stop`, which honours the file's `stop_grace_period` (elpida
     waits up to 300 s to save its world; a hard kill once lost 612 objects).
     Anvil never kills a container.
   - **Who started it (user's decision):** the project's containers are shown and
     manageable whoever started them (terminal or Anvil). Other Docker projects
     are not shown.

### Commands (one-off)

Commands always come from `package.json` (the scripts that aren't servers),
whatever source gave the rows; composer scripts are not Commands. `.anvil.toml`
`[processes]` keeps its format and only defines rows. (my call)

### Ports and addresses

- Only package.json server rows of non-PHP apps get the treatment below. Rows from
  Laravel's list, composer `dev` and `.anvil.toml` run exactly as written, as
  today, so Laravel's Vite keeps its hot-reload URL. (my call)
- Each such server row gets its own port from Anvil (as apps do today). Vite-family
  tools ignore `$PORT`, so Anvil appends `--port $PORT --strictPort --host
  127.0.0.1` for vite/vitepress/astro (as it already does for Vite).
- The main server row answers at `<app>.<tld>`; any other web server row gets
  `<row>.<app>.<tld>`, the row name cut at the first `:` (`docs:dev` →
  `docs.vuefinder.anvil`). (my call: the user said "continue" without choosing)
- Compose services publish their own ports; Anvil shows them and doesn't proxy.

## Part 2: the Processes window (task manager)

- A titlebar button between Settings and Serving opens a separate **Processes**
  window (focus if already open).
- It lists everything Anvil runs, grouped: **Web server** (Caddy and each PHP
  version's PHP-FPM, read-only: no Restart/Stop, the Serving control does that),
  **Applications** (each app as a header with its rows from Part 1 beneath),
  **Services** (MySQL, Postgres, Redis, Mailpit).
- Each row: name, PID, port, CPU %, memory, status, Restart and Stop.
- Selecting a row shows its log live in a panel at the bottom.
- A row that **crashes** (exits without being asked to) stays, red, with its log
  and Restart, until Stop dismisses it or a restart succeeds. Crash records are
  in memory only.
- Only what Anvil started, plus the project's own compose containers. Nothing
  unrelated (no Excel, Chrome, Herd).
- Applications group: only apps with a running or crashed row. Stopped apps don't
  appear; starting them stays in the main window. (my call)

## Non-goals

- Listing or killing processes Anvil didn't start.
- Running `php artisan dev`'s own TUI or `concurrently` as one process.
- Proxying compose services; managing images, volumes or builds.
- History, charts, search in the Processes window.

## Anti-goals

- Clutter (Anvil rule): no settings forest, no extra columns; green = running.
- A picker the user has to think about before anything runs.

## Constraints and existing parts to reuse

- `Apps` (`src/apps.rs`) runs children in their own process group with logs in
  `~/.anvil/log/app-<key>.log`; keys `<site>` and `<site>@<proc>`. It needs an
  accessor for running keys + pids and must record unexpected exits.
- `processes::detect`, `ProcessRow`, `process_run` (`app/src/main.rs`) and
  `app/ui/processes.js` already draw simple rows in the Overview; Part 1
  replaces/extends them.
- `resources::ps()` already reads one `ps -A` table (make it public) for CPU/RAM.
- `logs::tail(path, from)` works on any path; one new command maps an Anvil key
  (app, process, service, compose service, web server) to its log and tails it.
  It never takes a path from the UI.
- Settings window pattern: `open_settings` in `app/src/main.rs`; add the new
  window label to `app/capabilities/default.json`.

## Decided by the user

- Design C: every row with its own ▶/■/Restart, plus Start all; no selection step.
- Separate Processes window from a titlebar button between Settings and Serving.
- Contents: dev servers, app sub-processes, services; Caddy/PHP-FPM read-only.
- Crashed processes stay red with their log and Restart.
- Only what Anvil started.
- Laravel: not `artisan dev`'s TUI; Anvil runs the pieces itself.
- Compose containers outlive Anvil; stopped only by ■ / Stop all with the file's
  grace period; the project's containers show whoever started them.

## My calls (marked for review)

- Second web servers get `<row>.<app>.<tld>` addresses.
- Server-vs-command heuristic for package.json scripts (above).
- Laravel list read by booting the app with its PHP, cached, with fallback.
- Docker Compose becomes its own kind.
- Processes window ~820×560, list on top, log below; grouping as above.
