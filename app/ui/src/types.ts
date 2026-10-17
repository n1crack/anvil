// What the backend sends, mirroring the serde structs in app/src/main.rs and src/*.rs.
// Option<T> is `T | null`; maps are records.

export interface Site {
  name: string;
  host: string;
  secure: boolean;
  path: string;
  root: string;
  php: string | null;
  php_pinned: boolean;
  ini: Record<string, string>;
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
}

export interface Snapshot {
  sites: Site[];
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
  /** [id, label] */
  kinds: [string, string][];
  paths: string[];
  runtimes: Record<string, string[]>;
}

export interface Setup {
  resolver: boolean;
  trusted: boolean;
  php: boolean;
  ports: boolean;
  shims: boolean;
}

export interface Exit {
  code: number | null;
  /** Unix seconds. */
  at: number;
}

export interface Process {
  id: string;
  name: string;
  command: string;
  source: string;
  server: boolean;
  port: number | null;
  autostart: boolean;
  edited: boolean;
  pinned: boolean;
  top: boolean;
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
}

export interface Overview {
  rows: ProcessRow[];
  commands: CommandChip[];
  containers: Container[] | null;
  docker_error: string | null;
  hidden: { id: string; name: string }[];
}

export interface ActivityRow {
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

export interface Activity {
  web: ActivityRow[];
  apps: { site: string; rows: ActivityRow[] }[];
  services: ActivityRow[];
}

export interface ServiceInfo {
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
  php: { version: string; installed: boolean; default: boolean }[];
  ini: Record<string, string>;
  terminal: string | null;
  terminals: string[];
  git_client: string | null;
  git_clients: string[];
  theme: string | null;
  dock_icon: boolean;
  trim_app_logs: boolean;
  https_default: boolean;
  login: boolean;
  config_dir: string;
}

export interface LogSource {
  id: string;
  label: string;
  path: string;
  bytes: number;
  modified: number;
}

export interface LogEntry {
  time: string | null;
  env: string | null;
  level: string;
  message: string;
  context: string | null;
  trace: string[];
}

export interface LogPage {
  entries: LogEntry[];
  start_offset: number;
  end_offset: number;
  truncated: boolean;
}

export interface LogText {
  text: string;
  end_offset: number;
}
