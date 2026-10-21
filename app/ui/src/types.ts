// What the backend sends, mirroring the serde structs in app/src/main.rs and src/*.rs.
// Option<T> is `T | null`; maps are records.

export interface Site {
  name: string;
  host: string;
  secure: boolean;
  path: string;
  root: string;
  /** The web root the user set, relative to the project folder. */
  root_custom: string | null;
  /** The one Anvil picks without it: `public` when present, else `.`. */
  root_detected: string;
  php: string | null;
  php_pinned: boolean;
  ini: Record<string, string>;
  /** Its own extensions, on top of its version's. */
  ext: string[];
  /** Served by a PHP-FPM master of its own. */
  own_master: boolean;
  linked: boolean;
  parked_in: string | null;
  modified: number;
  kind: string;
  kind_detected: string;
  command: string | null;
  command_custom: boolean;
  port: number | null;
  autostart: boolean;
  runtime_version: string | null;
  runtime_pinned: boolean;
  /** The Settings tab's "Browser errors" switch. */
  browser: boolean;
  /** The Packages tab's badges; null is the detected ones. */
  packages_pinned: string[] | null;
  /** A pnpm app's "Shared dependencies" in effect; null without pnpm-lock.yaml. */
  shared_deps: boolean | null;
  /** For a git worktree of another app: that app's name. */
  worktree_of: string | null;
  /** A worktree's branch; null when detached. */
  branch: string | null;
  /** A worktree's addresses derived from its app's aliases. */
  aliases: string[];
}

export interface Snapshot {
  sites: Site[];
  /** Parked apps hidden from Anvil. */
  hidden: { name: string; path: string; kind: string }[];
  tld: string;
  https_port: number;
  http_port: number;
  php_default: string | null;
  php_installed: string[];
  editor: string | null;
  terminal: string | null;
  git_client: string | null;
  groups: Record<string, string[]>;
  ini: Record<string, string>;
  running: boolean;
  error: string | null;
  /** "starting" until the app's own server answers, "running" while anything of it runs
   * (server, dev process, container), "crashed" when one crashed and none runs. */
  app_states: Record<string, "starting" | "running" | "crashed">;
  app_errors: Record<string, string>;
  /** Only from set_app and set_runtime: whether the change restarted the app. */
  restarted?: boolean;
  /** [id, label] */
  kinds: [string, string][];
  paths: string[];
  runtimes: Record<string, string[]>;
  /** What an app's .anvil.toml asks for, by app name; apps with one only. */
  projects: Record<string, Need[]>;
  /** The Anvil Mailpit each app's .env sends mail to, by app name; apps that send there only. */
  mail: Record<string, string>;
  /** Settings › Features › Mail catcher while it's on: its port, why it isn't listening,
   * the apps whose mail it catches. */
  catcher: { port: number; error: string | null; caught: string[] } | null;
  /** Each worktree's database and what its Overview warns about, by worktree name. */
  worktrees: Record<string, WorktreeView>;
  /** Database copies of worktrees that are gone; never deleted on their own. */
  leftovers: Leftover[];
  /** The extensions Anvil can add. */
  php_ext_catalog: string[];
  /** What a catalog extension needs first that isn't here (Homebrew's ImageMagick), by name. */
  php_ext_needs?: Record<string, string>;
  /** Extensions on for every app on a version, by version. */
  php_ext: Record<string, string[]>;
  /** The extensions built for each version that has some on; one on without a build loads nothing. */
  php_ext_built: Record<string, string[]>;
  php_no_mysqli?: string[];
  /** Settings › Features › Tunnel: Overview offers Share. */
  tunnel: boolean;
  /** Settings › Features › Request log: apps' Requests tabs. */
  request_log: boolean;
  /** Shared apps' tunnels, by app; `note` says what links the app builds point to. */
  shares: Record<string, Share>;
  /** Settings › Features › Database viewer while it's on: why it isn't served, and each
   * app's database it opens, as the Overview names it. */
  db_viewer: { problem: string | null; apps: Record<string, string> } | null;
}

export type Share = ({ state: "starting" } | { state: "ready"; url: string } | { state: "failed"; error: string }) & {
  note: string | null;
};

export type DbChoice = "copy" | "empty";

/** One layer of an app's worktree hooks (worktrees::Hooks): a field left out is the next layer's. */
export interface Hooks {
  copy?: string[];
  copy_database?: boolean;
  keys?: Record<string, string>;
  check?: string[];
  after_create?: string[];
}

export interface WorktreeView {
  db: DbChoice;
  /** What it may choose now: what main's database allows (external: empty only)
   * until its own is made, then that one. */
  choices: DbChoice[];
  /** Its own database is made. */
  prepared: boolean;
  /** Its own database (a name, or a SQLite file in it); null when there's none to give it. */
  database: string | null;
  /** Main's, as "copy of …" names it. */
  main_database: string | null;
  /** Its own database is still to be made (Start, a terminal shim or Prepare database). */
  pending: boolean;
  /** "Anvil: preparing …" while a copy is made. */
  preparing: string | null;
  compose: boolean;
  /** bootstrap/cache/config.php exists: its env is ignored. */
  cached_config: boolean;
  /** Why nothing of it starts. */
  error: string | null;
  /** Databases Anvil made for it on a server. */
  copies: string[];
  /** Its database was there already; the user chose to use it. */
  adopted: boolean;
  /** What it lacks to run: the files its hooks copy, and packages to install as [id, command]. */
  missing: { copy: string[]; installs: [string, string][] };
  /** The hooks' check keys its env files still give main's value. */
  same: string[];
}

export interface Leftover {
  app: string;
  worktree: string;
  service: string;
  database: string;
}

/** One thing a project's .anvil.toml asks for. */
export interface Need {
  /** "php", "node", "ext" (a PHP extension) or a service kind. */
  kind: string;
  /** As the file says it; "" is any version. */
  version: string;
  /** What installing or adding gets. */
  install: string;
  /** "PHP 8.3", "Redis". */
  label: string;
  met: boolean;
  /** The version chosen in Anvil instead. */
  overridden: string | null;
}

export interface Setup {
  resolver: boolean;
  trusted: boolean;
  php: boolean;
  ports: boolean;
  shims: boolean;
}

interface Exit {
  code: number | null;
  /** Unix seconds. */
  at: number;
}

interface Process {
  id: string;
  name: string;
  command: string;
  source: string;
  server: boolean;
  port: number | null;
  autostart: boolean;
  edited: boolean;
  /** Left out of Dev start. */
  manual: boolean;
  /** Added in Anvil, not detected. */
  added: boolean;
}

export interface ProcessRow extends Process {
  kind: "server" | "site" | "process";
  main: boolean;
  web: string | null;
  running: boolean;
  /** Running, its port not answering yet. */
  starting: boolean;
  last: string | null;
  error: string | null;
  address: string | null;
  host_edited: boolean;
  pid: number | null;
  crashed: Exit | null;
}

export interface CommandChip {
  name: string;
  id: string;
  command: string;
  source: "package.json" | "composer.json";
  running: boolean;
  exit: Exit | null;
}

export interface Container {
  service: string;
  state: string;
  health: string | null;
  ports: number[];
  /** How an exited container ended. */
  exit_code: number | null;
}

export interface Overview {
  rows: ProcessRow[];
  commands: CommandChip[];
  containers: Container[] | null;
  docker_error: string | null;
  compose_file?: string | null;
  hidden: { id: string; name: string }[];
}

interface ActivityRow {
  key: string;
  name: string;
  pid: number | null;
  port: number | null;
  cpu: number | null;
  memory: number | null;
  state: "running" | "starting" | "crashed";
  detail: string | null;
  actions: boolean;
  web: string | null;
}

/** What a client did through the control socket (app/src/control.rs). */
export interface AgentAction {
  at: number; // unix seconds
  client: string; // "Claude Code", or "Terminal" for the CLI
  site: string | null;
  what: string; // "restarted api"
  ok: boolean;
  detail: string | null; // the error, when it failed
}

export interface Activity {
  web: ActivityRow[];
  apps: { site: string; rows: ActivityRow[] }[];
  services: ActivityRow[];
  /** Why Docker couldn't list the containers, when an app has a compose file. */
  docker_error: string | null;
}

interface ServiceInfo {
  id: string;
  kind: string;
  name: string;
  category: string;
  version: string;
  port: number;
  web: string | null;
  web_port: number | null;
  installed: boolean;
  enabled: boolean;
  running: boolean;
  tags: string[];
  apps: string[];
  env: string;
  log: string;
  suggested_port: number | null;
  suggested_web_port: number | null;
}

export interface ServiceRow extends ServiceInfo {
  error: string | null;
  clients: string[];
}

export interface Settings {
  paths: string[];
  hidden: string[];
  tld: string;
  http_port: number;
  https_port: number;
  dns_port: number;
  editor: string | null;
  editors: string[];
  php: {
    version: string;
    installed: boolean;
    default: boolean;
    pinned_by: string[];
    opcache: "builtin" | "ready" | "missing";
    opcache_building: boolean;
    /** The catalog, each on or off for every app on the version. */
    /** `needs`: what it needs first that isn't here; it can't be turned on. */
    extensions: { name: string; on: boolean; built: boolean; building: boolean; needs?: string | null }[];
    /** A newer patch, [installed, newest], as the last check found. */
    available: [string, string] | null;
    /** The patch it was updated to since Anvil started. */
    updated: string | null;
    updating: boolean;
  }[];
  /** Settings › PHP › Keep PHP up to date. */
  php_auto_update: boolean;
  ini: Record<string, string>;
  terminal: string | null;
  terminals: string[];
  git_client: string | null;
  git_clients: string[];
  theme: string | null;
  dock_icon: boolean;
  trim_app_logs: boolean;
  notifications: boolean;
  https_default: boolean;
  worktree_db: DbChoice;
  /** Settings › Features › Agents (MCP). */
  mcp: boolean;
  /** Settings › Features › Tunnel. */
  tunnel: boolean;
  /** Settings › Features › Request log. */
  request_log: boolean;
  /** Settings › Features › Mail catcher, its port and why it isn't listening. */
  mail_catcher: boolean;
  mail_port: number;
  mail_error: string | null;
  /** Settings › Features › Database viewer, where it answers, and why it isn't served. */
  db_viewer: boolean;
  db_viewer_host: string;
  db_viewer_problem: string | null;
  /** Settings › Worktrees: each preset as it applies. */
  worktree_presets: { id: string; name: string; kinds: string[]; hooks: Hooks; edited: boolean }[];
  login: boolean;
  config_dir: string;
}
