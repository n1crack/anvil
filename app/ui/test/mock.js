// A mocked Tauri backend for the UI tests: window.__TAURI__ plus sample data.
window.mockCalls = [];
window.mockMenu = null;
const mockSite = (name, kind, extra = {}) => ({
  name,
  host: `${name}.test`,
  secure: true,
  path: `/Users/x/Sites/${name}`,
  root: `/Users/x/Sites/${name}/public`,
  php: kind === "laravel" ? "8.4" : null,
  php_pinned: false,
  ini: {},
  linked: false,
  parked_in: "/Users/x/Sites",
  modified: 0,
  kind,
  kind_detected: kind,
  command: null,
  command_custom: false,
  port: null,
  autostart: false,
  runtime_version: kind === "laravel" ? null : "22.1.0",
  runtime_pinned: false,
  ...extra,
});
window.mockSnap = {
  sites: [
    mockSite("shop", "laravel"),
    mockSite("admin.shop", "laravel", { linked: true, path: "/Users/x/Sites/shop", parked_in: null }),
    mockSite("api.shop", "laravel", { linked: true, path: "/Users/x/Sites/shop", parked_in: null, secure: false }),
    mockSite("notes", "folder"),
    mockSite("vuefinder", "vite", { command: "npm run dev", port: 5173 }),
  ],
  tld: "test",
  https_port: 443,
  http_port: 80,
  php_default: "8.4",
  php_installed: ["8.4"],
  editor: "Zed",
  git_client: null,
  groups: {},
  ini: {},
  running: true,
  error: null,
  app_states: {},
  app_errors: {},
  kinds: [
    ["laravel", "Laravel"],
    ["vite", "Vite"],
    ["folder", "Folder"],
  ],
  paths: ["/Users/x/Sites"],
  runtimes: { node: ["22.1.0"] },
};
const mockRow = (o) => ({
  id: "",
  name: "",
  command: "",
  source: "package.json",
  server: false,
  port: null,
  autostart: false,
  edited: false,
  pinned: false,
  kind: "process",
  main: false,
  web: null,
  running: false,
  last: null,
  error: null,
  address: null,
  pid: null,
  crashed: null,
  ...o,
});
window.mockOv = {
  notes: { rows: [], commands: [], containers: null, docker_error: null, hidden: [] },
  shop: {
    rows: [
      mockRow({ name: "shop.test", source: "anvil", server: true, kind: "site", main: true, web: "https://shop.test", running: true }),
      mockRow({ id: "queue", name: "queue", command: "php artisan queue:listen --tries=1", source: "artisan dev", running: true, pid: 11 }),
      mockRow({ id: "logs", name: "logs", command: "php artisan pail --timeout=0", source: "artisan dev", crashed: { code: 1, at: 1 } }),
      mockRow({ id: "vite", name: "vite", command: "npm run dev", source: "artisan dev", server: true, port: 5174, autostart: true }),
    ],
    commands: [
      { name: "build", id: "build", command: "vite build", running: false, exit: null },
      { name: "test:unit", id: "test-unit", command: "vitest", running: false, exit: { code: 0, at: 1 } },
    ],
    containers: null,
    docker_error: null,
    hidden: [{ id: "horizon", name: "horizon" }],
  },
  vuefinder: {
    rows: [
      mockRow({
        name: "dev",
        command: "npm run dev",
        source: "anvil",
        server: true,
        port: 5173,
        kind: "server",
        main: true,
        web: "https://vuefinder.test",
        running: false,
      }),
      mockRow({
        id: "docs-dev",
        name: "docs:dev",
        command: "npm run docs:dev",
        server: true,
        port: 5180,
        web: "https://docs.vuefinder.test",
        address: "https://docs.vuefinder.test",
        running: true,
      }),
    ],
    commands: [
      ["build", "vite build"],
      ["build-only", "vite build --mode lib"],
      ["lint", "oxlint && eslint . --fix --cache --max-warnings 0"],
      ["test", "vitest run"],
      ["typecheck", "vue-tsc --noEmit -p tsconfig.app.json"],
      ["docs:build", "npm run build && vitepress build docs"],
      ["preview", "vite preview"],
      ["format", "prettier --write src/"],
    ].map(([name, command]) => ({ name, id: name.replace(":", "-"), command, running: false, exit: null })),
    containers: null,
    docker_error: null,
    hidden: [],
  },
};
const mockLog = "\x1b[1m\x1b[32m✓\x1b[0m built in \x1b[33m1.2s\x1b[39m\n12%\r50%\r100%\n\x1b[2Kplain \x1b[91mbright red\x1b[0m\n";
{
  const c = mockOv.vuefinder.commands;
  const t = Math.floor(Date.now() / 1000);
  c[6].exit = { code: 0, at: t - 130 };
  c[7].exit = { code: 1, at: t - 3700 };
  c[3].running = true;
}
mockOv.vuefinder.rows.push(
  mockRow({
    id: "docs-site",
    name: "docs:site",
    command: "npm run docs:site",
    server: true,
    port: 5181,
    web: "https://docs-site.vuefinder.test",
    address: "https://docs-site.vuefinder.test",
  }),
);
window.mockServices = [
  {
    id: "mysql",
    kind: "mysql",
    name: "MySQL",
    category: "Databases",
    version: "8.4.2",
    port: 3306,
    web: null,
    web_port: null,
    installed: true,
    enabled: true,
    running: true,
    tags: ["main"],
    apps: ["shop"],
    env: "DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT=3306\nDB_USERNAME=root\nDB_PASSWORD=",
    log: "/x",
    suggested_port: null,
    suggested_web_port: null,
    clients: ["TablePlus"],
  },
  {
    id: "postgres",
    kind: "postgres",
    name: "PostgreSQL",
    category: "Databases",
    version: "17.2",
    port: 5432,
    web: null,
    web_port: null,
    installed: true,
    enabled: false,
    running: false,
    tags: [],
    apps: [],
    env: "DATABASE_URL=postgres://postgres@127.0.0.1:5432/postgres",
    log: "/x",
    suggested_port: null,
    suggested_web_port: null,
    clients: [],
  },
  {
    id: "cache",
    kind: "redis",
    name: "Redis",
    category: "Key-value",
    version: "7.4.1",
    port: 6379,
    web: null,
    web_port: null,
    installed: true,
    enabled: true,
    running: true,
    tags: ["queue", "cache"],
    apps: ["shop"],
    env: "REDIS_HOST=127.0.0.1\nREDIS_PORT=6379",
    log: "/x",
    suggested_port: null,
    suggested_web_port: null,
    clients: [],
  },
  {
    id: "mailpit",
    kind: "mailpit",
    name: "Mailpit",
    category: "Mail",
    version: "1.21",
    port: 1025,
    web: "http://127.0.0.1:8025",
    web_port: 8025,
    installed: true,
    enabled: true,
    running: false,
    tags: [],
    apps: [],
    env: "MAIL_HOST=127.0.0.1\nMAIL_PORT=1025",
    log: "/x",
    suggested_port: null,
    suggested_web_port: null,
    clients: [],
  },
];
const mockHandle = (cmd, a) => {
  const ov = a?.site && mockOv[a.site];
  switch (cmd) {
    case "state":
    case "app_run":
      return mockSnap;
    case "setup_status":
      return window.mockSetup ?? { resolver: true, ports: true, trusted: true, php: true, shims: true };
    case "setup_fix": {
      if (a.step === "all") Object.assign(window.mockSetup, { resolver: true, ports: true, shims: true });
      if (a.step === "trust") window.mockSetup.trusted = true;
      return window.mockSetup;
    }
    case "processes":
    case "process_run":
    case "containers_run":
      return ov;
    case "command_run":
      ov.commands.find((c) => c.name === a.script).running = true;
      return ov;
    case "set_process": {
      if (a.command === "") throw "The command can't be empty";
      const r = ov.rows.find((x) => x.id === a.id);
      if (a.host != null) {
        if (a.host === "docs-site") throw "docs-site.vuefinder.test is already the address of docs:site";
        Object.assign(r, { address: `https://${a.host}.vuefinder.test`, web: `https://${a.host}.vuefinder.test`, host_edited: true });
      }
      if (a.command != null) Object.assign(r, { command: a.command, edited: true });
      if (a.autostart != null) r.autostart = a.autostart;
      if (a.top != null) {
        r.top = a.top;
        ov.rows.sort((x, y) => y.main - x.main || !!y.top - !!x.top);
      }
      if (a.hidden === true) {
        ov.rows = ov.rows.filter((x) => x !== r);
        ov.hidden.push({ id: r.id, name: r.name });
      }
      if (a.hidden === false) {
        const h = ov.hidden.find((x) => x.id === a.id);
        ov.hidden = ov.hidden.filter((x) => x !== h);
        ov.rows.push(mockRow({ id: h.id, name: h.name, command: "php artisan horizon" }));
      }
      if (a.pinned === true) {
        const c = ov.commands.find((x) => x.id === a.id);
        ov.commands = ov.commands.filter((x) => x !== c);
        ov.rows.push(mockRow({ id: c.id, name: c.name, command: `npm run ${c.name}`, pinned: true }));
      }
      return structuredClone(ov);
    }
    case "log_text": {
      const text = window.mockLogs?.[a.source] ?? mockLog;
      return { text: a.from ? "" : text, end_offset: text.length };
    }
    case "log_sources":
      return [];
    case "services":
    case "service_run":
      return mockServices;
    case "service_log":
      return [
        "2026-10-17T10:00:01 [Note] mysqld: ready for connections.",
        "2026-10-17T10:00:01 [Note] Version: '8.4.2'  socket: '/tmp/mysql.sock'  port: 3306",
      ];
    case "service_contents":
      return {
        databases: [
          ["shop", 52428800],
          ["blog", 1048576],
          ["mysql", 2621440],
          ["sys", 40960],
        ],
        keys: [["db0", 12]],
        memory: "1.2 MB",
        messages: 3,
      };
    case "once_stop":
      ov.commands.find((c) => c.id === a.id).running = false;
      return null;
    case "link_folder":
      return "notes";
    case "create_app":
      return "notes";
    case "touched":
      return Math.floor(Date.now() / 1000) - 7200;
    case "git_info":
      return { branch: "main", detached: false, ahead: 0, behind: 0, changed: 3, upstream: null, commit: null, remote: null };
    case "set_secure": {
      const s = mockSnap.sites.find((x) => x.name === a.site);
      s.secure = a.on;
      return mockSnap;
    }
    default:
      return null;
  }
};
window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      mockCalls.push([cmd, args]);
      const ms = window.mockDelay?.[cmd];
      if (ms) await new Promise((r) => setTimeout(r, ms));
      return structuredClone(mockHandle(cmd, args));
    },
  },
  event: { listen: async () => () => {} },
  menu: {
    Menu: {
      new: async ({ items }) => ({
        items,
        popup() {
          window.mockMenu = items;
        },
      }),
    },
    MenuItem: { new: async (o) => o },
    CheckMenuItem: { new: async (o) => o },
    PredefinedMenuItem: { new: async (o) => o },
    Submenu: { new: async (o) => o },
  },
  dpi: {
    LogicalPosition: class {
      constructor(x, y) {
        this.x = x;
        this.y = y;
      }
    },
  },
  dialog: { ask: async () => false, message: async () => {}, open: async () => null },
};
