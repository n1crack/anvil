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
// Logs of a Laravel shop, close to a real one.
{
  const now = Date.now();
  const ts = (minAgo) =>
    new Date(now - minAgo * 60000 - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 19).replace("T", " ");
  const vend = (n) =>
    `#${n} /var/www/shop/vendor/laravel/framework/src/Illuminate/${["Container/Container.php(950): Illuminate\\Container\\Container->build()", "Container/Container.php(821): Illuminate\\Container\\Container->resolve()", "Foundation/Application.php(1078): Illuminate\\Container\\Container->resolve()", "Pipeline/Pipeline.php(183): Illuminate\\Pipeline\\Pipeline->Illuminate\\Pipeline\\{closure}()", "Routing/Router.php(799): Illuminate\\Routing\\Pipeline->then()"][n % 5]}`;
  const trace = [];
  for (let i = 0; i < 59; i++) {
    if (i === 0)
      trace.push(
        "#0 /var/www/shop/vendor/laravel/framework/src/Illuminate/Container/Container.php(1019): Illuminate\\Container\\Container->notInstantiable()",
      );
    else if (i === 7) trace.push("#7 /var/www/shop/app/Http/Controllers/CheckoutController.php(42): request()");
    else if (i === 8)
      trace.push("#8 /var/www/shop/app/Http/Middleware/EnsureCartNotEmpty.php(23): App\\Http\\Middleware\\EnsureCartNotEmpty->handle()");
    else if (i === 30) trace.push("#30 /var/www/shop/app/Providers/RouteServiceProvider.php(51): Illuminate\\Routing\\Router->group()");
    else if (i === 58) trace.push("#58 {main}");
    else trace.push(vend(i).replace(/^#\d+/, "#" + i));
  }
  const E = (min, level, message, extra = {}) => ({ time: ts(min), env: "local", level, message, context: null, trace: [], ...extra });
  const err = "Target class [request] does not exist.";
  const entries = [
    E(1, "ERROR", err, { trace }),
    E(2, "ERROR", err, { trace }),
    E(2, "ERROR", err, { trace }),
    E(4, "ERROR", err, { trace }),
    E(6, "INFO", "Order placed", {
      context: JSON.stringify({
        order_id: 10482,
        user_id: 37,
        total: 129.9,
        currency: "EUR",
        items: [
          { sku: "TS-001", qty: 2 },
          { sku: "MG-014", qty: 1 },
        ],
      }),
    }),
    E(8, "WARNING", "Cache store [redis] unavailable, falling back to [file]"),
    E(9, "DEBUG", "select * from `users` where `id` = ? limit 1 [37] (1.24ms)"),
    E(9, "DEBUG", "select * from `orders` where `user_id` = ? [37] (0.88ms)"),
    E(11, "ERROR", "SQLSTATE[HY000] [2002] Connection refused (Connection: mysql, SQL: select * from `sessions` where `id` = ?)", {
      trace: trace.slice(0, 12),
    }),
    E(14, "INFO", "Queue worker started", { context: JSON.stringify({ queue: "default", tries: 1 }) }),
    E(15, "DEBUG", "Mail sent to anna@example.com (subject: Your receipt)"),
    E(19, "WARNING", "Deprecated: Creation of dynamic property App\\Models\\Cart::$coupon is deprecated"),
    E(22, "DEBUG", "Stripe webhook payload received [charge.succeeded]"),
    E(25, "INFO", "User logged in", { context: JSON.stringify({ user_id: 37, ip: "127.0.0.1" }) }),
    E(31, "DEBUG", "select count(*) as aggregate from `products` where `active` = ? [1] (2.01ms)"),
    E(33, "ERROR", err, { trace }),
    E(36, "ERROR", err, { trace }),
    E(41, "NOTICE", "Route [shop.cart] is slow: 842ms"),
    E(47, "DEBUG", "View cache cleared"),
    E(52, "INFO", "Scheduled task finished", { context: JSON.stringify({ command: "inspire", ms: 31 }) }),
    E(58, "DEBUG", "Session regenerated"),
    E(64, "WARNING", 'Undefined array key "discount" in app/Services/PriceCalculator.php:88'),
    E(70, "DEBUG", "Config cached"),
    E(75, "INFO", "Application booted", { context: null }),
    E(80, "DEBUG", "Vite manifest read (3 entries)"),
  ];
  const page = { entries, start_offset: 0, end_offset: 2097152, truncated: false };
  const sources = [
    {
      id: "laravel.log",
      label: "laravel.log",
      path: "/Users/x/Sites/shop/storage/logs/laravel.log",
      bytes: 2097152,
      modified: Math.floor(now / 1000) - 60,
    },
    { id: "once:build", label: "build", path: "/tmp/build.log", bytes: 4210, modified: Math.floor(now / 1000) - 3600 },
    {
      id: "php-fpm",
      label: "PHP-FPM 8.4",
      path: "/Users/x/.anvil/log/php-8.4-fpm.log",
      bytes: 18200,
      modified: Math.floor(now / 1000) - 7200,
    },
  ];
  window.mockLogSources = sources;
  window.mockLogPage = page;
}
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
      return window.mockLogSources;
    case "log_read":
    case "log_search":
      return window.mockLogPage;
    case "log_tail":
      return { entries: window.mockTail?.splice(0) ?? [], start_offset: 0, end_offset: window.mockLogPage.end_offset, truncated: false };
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
    case "settings":
      return {
        paths: ["/Users/x/Sites"],
        hidden: ["old-blog"],
        tld: "test",
        http_port: 80,
        https_port: 443,
        dns_port: 53,
        editor: "Zed",
        editors: ["Zed", "Visual Studio Code"],
        php: [
          { version: "8.4", installed: true, default: true },
          { version: "8.3", installed: true, default: false },
          { version: "8.2", installed: false, default: false },
        ],
        ini: { memory_limit: "512M", upload_max_filesize: "64M" },
        terminal: "Terminal",
        terminals: ["Terminal", "Ghostty"],
        git_client: null,
        git_clients: ["Fork"],
        theme: null,
        dock_icon: true,
        https_default: true,
        login: false,
        config_dir: "/Users/x/Library/Application Support/Anvil",
      };
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
