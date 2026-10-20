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
  worktree_of: null,
  branch: null,
  aliases: [],
  ...extra,
});
window.mockSite = mockSite;
window.mockSnap = {
  sites: [
    mockSite("shop", "laravel"),
    mockSite("admin.shop", "laravel", { linked: true, path: "/Users/x/Sites/shop", parked_in: null }),
    mockSite("api.shop", "laravel", { linked: true, path: "/Users/x/Sites/shop", parked_in: null, secure: false }),
    mockSite("notes", "folder"),
    mockSite("vuefinder", "vite", { command: "npm run dev", port: 5173 }),
    mockSite("stack", "compose", { secure: false, runtime_version: null }),
  ],
  hidden: [{ name: "old-blog", path: "/Users/x/Sites/old-blog", kind: "laravel" }],
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
    ["compose", "Docker Compose"],
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
  top: false,
  manual: false,
  added: false,
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
const mockContainer = (service, state, o = {}) => ({ service, state, health: null, ports: [], exit_code: null, ...o });
window.mockOv = {
  notes: { rows: [], commands: [], containers: null, docker_error: null, hidden: [] },
  // A Docker Compose project: up, healthy, still starting and one that exited with an error.
  stack: {
    rows: [],
    commands: [],
    containers: [
      mockContainer("web", "running", { ports: [8080] }),
      mockContainer("db", "running", { health: "healthy", ports: [5432] }),
      mockContainer("cache", "running", { health: "starting", ports: [6379] }),
      mockContainer("worker", "exited", { exit_code: 1 }),
    ],
    docker_error: null,
    hidden: [],
  },
  shop: {
    rows: [
      mockRow({ name: "shop.test", source: "anvil", server: true, kind: "site", main: true, web: "https://shop.test", running: true }),
      mockRow({ id: "queue", name: "queue", command: "php artisan queue:listen --tries=1", source: "artisan dev", running: true, pid: 11 }),
      mockRow({ id: "logs", name: "logs", command: "php artisan pail --timeout=0", source: "artisan dev", crashed: { code: 1, at: 1 } }),
      mockRow({ id: "vite", name: "vite", command: "npm run dev", source: "artisan dev", server: true, port: 5174, autostart: true }),
    ],
    commands: [
      { name: "build", id: "build", command: "vite build", source: "package.json", running: false, exit: null },
      { name: "test:unit", id: "test-unit", command: "vitest", source: "package.json", running: false, exit: { code: 0, at: 1 } },
      {
        name: "test",
        id: "composer-test",
        command: "@php artisan config:clear && @php artisan test",
        source: "composer.json",
        running: false,
        exit: null,
      },
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
        web: "https://vuefinder--docs.test",
        address: "https://vuefinder--docs.test",
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
    ].map(([name, command]) => ({ name, id: name.replace(":", "-"), command, source: "package.json", running: false, exit: null })),
    containers: null,
    docker_error: null,
    hidden: [],
  },
};
const mockLog = "\x1b[1m\x1b[32m✓\x1b[0m built in \x1b[33m1.2s\x1b[39m\n12%\r50%\r100%\n\x1b[2Kplain \x1b[91mbright red\x1b[0m\n";
// `vite build` as a once-run writes it: colors, no timestamps nor levels.
const mockBuild = [
  "",
  "> shop@ build",
  "> vite build",
  "",
  "\x1b[36mvite v6.2.0 \x1b[32mbuilding for production...\x1b[39m",
  "transforming...",
  "\x1b[32m✓\x1b[39m 412 modules transformed.",
  "\x1b[33m(!) Some chunks are larger than 500 kB after minification.\x1b[39m",
  "rendering chunks...",
  "computing gzip size...",
  "\x1b[2mpublic/build/\x1b[22m\x1b[32mmanifest.json            \x1b[39m\x1b[1m\x1b[2m  0.62 kB\x1b[22m\x1b[1m\x1b[22m",
  "\x1b[2mpublic/build/\x1b[22m\x1b[35massets/app-BqW2c9xP.css  \x1b[39m\x1b[1m\x1b[2m 48.20 kB\x1b[22m\x1b[1m\x1b[22m\x1b[2m │ gzip:  8.91 kB\x1b[22m",
  "\x1b[2mpublic/build/\x1b[22m\x1b[36massets/app-D1x9fK2a.js   \x1b[39m\x1b[1m\x1b[33m612.44 kB\x1b[39m\x1b[22m\x1b[2m │ gzip: 196.03 kB\x1b[22m",
  "\x1b[32m✓ built in 3.41s\x1b[39m",
  "",
  "> shop@ build:ssr",
  "> vite build --ssr",
  "",
  "\x1b[31merror\x1b[39m during build:",
  '\x1b[31m[vite]: Rollup failed to resolve import "@/Components/Chart" from "resources/js/Pages/Dashboard.vue".\x1b[39m',
  "npm \x1b[31mERR!\x1b[39m code 1",
  "npm \x1b[31mERR!\x1b[39m path /Users/x/Sites/shop",
  "npm \x1b[31mERR!\x1b[39m command failed",
  "",
].join("\n");
// What `php artisan dev`'s processes print in a terminal, colors and all (FORCE_COLOR, --ansi).
const E = "\x1b[";
const mockSiteLogs = {
  shop: {
    "proc:queue": `\n  ${E}37;44m INFO ${E}39;49m Processing jobs from the [${E}33mdefault${E}39m] queue.\n\n  2026-10-17 10:00:04 App\\Jobs\\SendInvoice ${E}90m.......${E}39m ${E}33mRUNNING${E}39m\n  2026-10-17 10:00:04 App\\Jobs\\SendInvoice ${E}90m.......${E}39m 38.12ms ${E}32;1mDONE${E}39;22m\n  2026-10-17 10:00:09 App\\Jobs\\SyncStock ${E}90m..........${E}39m 1s ${E}31;1mFAIL${E}39;22m\n`,
    "proc:logs": `\n  ${E}37;44m INFO ${E}39;49m Tailing application logs. ${E}90mPress Ctrl+C to exit${E}39m\n\n  ${E}30;46m 10:00:09 ${E}39;49m ${E}37;41m ERROR ${E}39;49m Stock service timed out ${E}90mapp/Jobs/SyncStock.php:41${E}39m\n  ${E}30;46m 10:00:12 ${E}39;49m ${E}30;43m WARNING ${E}39;49m Cache miss rate above 40% ${E}90mapp/Http/Middleware/Cache.php:22${E}39m\n`,
    "proc:vite": `\n> dev\n> vite\n\n\n  ${E}32m${E}1mVITE${E}22m v7.1.4${E}39m  ${E}2mready in ${E}0m${E}1m412${E}22m${E}2m${E}0m ms${E}22m\n\n  ${E}32m➜${E}39m  ${E}1mLocal${E}22m:   ${E}36mhttp://localhost:${E}1m5174${E}22m/${E}39m\n  ${E}32m➜${E}39m  ${E}1mNetwork${E}22m: ${E}2muse ${E}22m${E}1m--host${E}22m${E}2m to expose${E}22m\n\n  ${E}1mLARAVEL${E}22m ${E}36mplugin v2.0.1${E}39m\n\n  ${E}32m➜${E}39m  ${E}36mAPP_URL${E}39m: ${E}36mhttps://shop.test${E}39m\n${E}2m10:00:21 AM${E}22m ${E}36m${E}1m[vite]${E}22m${E}39m ${E}32mhmr update ${E}39m${E}2m/resources/js/app.js${E}22m\n`,
  },
};
// The Processes window's log: a process's colored output, or a service's plain timestamped lines.
const mockActivityLogs = {
  "shop@queue": mockSiteLogs.shop["proc:queue"],
  "shop@vite": mockSiteLogs.shop["proc:vite"],
  "service:mysql":
    "2026-10-17T10:00:01.112Z 0 [System] [MY-010116] mysqld (mysqld 8.4.2) starting as process 801\n2026-10-17T10:00:01.420Z 0 [Warning] [MY-010068] CA certificate ca.pem is self signed.\n2026-10-17T10:00:01.602Z 0 [System] [MY-010931] mysqld: ready for connections. port: 3306\n",
  "web:php-8.4":
    "[17-Oct-2026 10:00:00] NOTICE: fpm is running, pid 412\n[17-Oct-2026 10:00:00] NOTICE: ready to handle connections\n[17-Oct-2026 10:04:12] WARNING: [pool www] server reached pm.max_children setting (5)\n[17-Oct-2026 10:05:40] ERROR: unable to read what child say: Bad file descriptor (9)\n",
};
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
    web: "https://vuefinder--docs-site.test",
    address: "https://vuefinder--docs-site.test",
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
// Logs and packages of a Laravel shop, close to a real one.
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
  // The same exception 24 times over three hours, as Laravel writes it: the context only
  // repeats it, and no frame of the trace is the app's.
  const ill = "/Users/x/Sites/shop/vendor/laravel/framework/src/Illuminate/";
  const steps = [
    ["Container/Container.php(1127)", "Illuminate\\Container\\Container->build()"],
    ["Container/Container.php(950)", "Illuminate\\Container\\Container->resolve()"],
    ["Foundation/Application.php(1078)", "Illuminate\\Foundation\\Application->resolve()"],
    ["Pipeline/Pipeline.php(183)", "Illuminate\\Pipeline\\Pipeline->Illuminate\\Pipeline\\{closure}()"],
    ["Routing/Router.php(799)", "Illuminate\\Routing\\Router->runRouteWithinStack()"],
  ];
  const vendorTrace = Array.from({ length: 60 }, (_, i) => `#${i} ${ill}${steps[i % 5][0]}: ${steps[i % 5][1]}`);
  const binding = {
    context: JSON.stringify({
      exception: `[object] (Illuminate\\Contracts\\Container\\BindingResolutionException(code: 0): ${err} at ${ill}Container/Container.php:1127)`,
    }),
    trace: vendorTrace,
  };
  // A browser.log entry: React's hydration error with its tree, as the browser reports it.
  const hydration = [
    "Uncaught Error: Hydration failed because the server rendered HTML didn't match the client. As a result this tree will be regenerated on the client. This can happen if a SSR-ed Client Component used:",
    "",
    "- A server/client branch `if (typeof window !== 'undefined')`.",
    "- Variable input such as `Date.now()` or `Math.random()` which changes each time it's called.",
    "",
    "https://react.dev/link/hydration-mismatch",
    "",
    "  <App>",
    '    <Layout title="Orders">',
    "      <Table>",
    '        <div className="table-wrap" style={{...}}>',
    "          <table>",
    "            ...",
    "              <tr>",
    '+               <td className="cell">',
    '-               <td className="cell cell-active">',
    '>                <span title="%s" data-x="<img src=x onerror=alert(1)>">{"Total"}</span>',
  ].join("\n");
  const entries = [
    ...Array.from({ length: 24 }, (_, i) => E(1 + i * 8, "ERROR", err, binding)),
    E(200, "INFO", "Horizon started"),
    E(201, "ERROR", err, { trace }),
    E(202, "ERROR", err, { trace }),
    E(202, "ERROR", err, { trace }),
    E(204, "ERROR", err, { trace }),
    E(206, "INFO", "Order placed", {
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
    E(208, "WARNING", "Cache store [redis] unavailable, falling back to [file]"),
    E(209, "DEBUG", "select * from `users` where `id` = ? limit 1 [37] (1.24ms)"),
    E(209, "DEBUG", "select * from `orders` where `user_id` = ? [37] (0.88ms)"),
    E(211, "ERROR", "SQLSTATE[HY000] [2002] Connection refused (Connection: mysql, SQL: select * from `sessions` where `id` = ?)", {
      trace: trace.slice(0, 12),
    }),
    E(214, "INFO", "Queue worker started", { context: JSON.stringify({ queue: "default", tries: 1 }) }),
    E(215, "DEBUG", "Mail sent to anna@example.com (subject: Your receipt)"),
    E(219, "WARNING", "Deprecated: Creation of dynamic property App\\Models\\Cart::$coupon is deprecated"),
    E(222, "DEBUG", "Stripe webhook payload received [charge.succeeded]"),
    E(225, "INFO", "User logged in", { context: JSON.stringify({ user_id: 37, ip: "127.0.0.1" }) }),
    E(231, "DEBUG", "select count(*) as aggregate from `products` where `active` = ? [1] (2.01ms)"),
    E(233, "ERROR", err, { trace }),
    E(236, "ERROR", err, { trace }),
    E(241, "NOTICE", "Route [shop.cart] is slow: 842ms"),
    E(247, "DEBUG", "View cache cleared"),
    E(252, "INFO", "Scheduled task finished", { context: JSON.stringify({ command: "inspire", ms: 31 }) }),
    E(258, "DEBUG", "Session regenerated"),
    E(264, "WARNING", 'Undefined array key "discount" in app/Services/PriceCalculator.php:88'),
    E(270, "DEBUG", "Config cached"),
    E(275, "INFO", "Application booted", { context: null }),
    E(280, "DEBUG", "Vite manifest read (3 entries)"),
    E(290, "ERROR", hydration),
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
  const P = (name, constraint, version, dev, released) => ({ name, constraint, version, dev, released: released ?? null });
  const composer = {
    id: "composer",
    label: "Composer",
    manager: "composer 2.8.4",
    lock: "composer.lock",
    facts: [["PHP", "^8.2"]],
    packages: [
      P("laravel/framework", "^11.31", "11.36.1", false, "2024-12-17"),
      P("laravel/tinker", "^2.9", "2.10.0", false, "2024-09-19"),
      P("laravel/sanctum", "^4.0", "4.0.7", false, "2024-12-11"),
      P("livewire/livewire", "^3.5", "3.5.18", false, "2024-12-12"),
      P("guzzlehttp/guzzle", "^7.9", "7.9.2", false, "2024-07-24"),
      P("spatie/laravel-permission", "^6.10", "6.10.1", false, "2024-11-07"),
      P("stripe/stripe-php", "^15.0", "15.12.0", false, "2024-11-27"),
      P("league/flysystem-aws-s3-v3", "^3.0", "3.29.0", false, "2024-10-08"),
      P("fakerphp/faker", "^1.23", "1.24.1", true, "2024-11-19"),
      P("laravel/pint", "^1.13", "1.18.3", true, "2024-12-06"),
      P("laravel/sail", "^1.26", "1.39.1", true, "2024-12-11"),
      P("mockery/mockery", "^1.6", "1.6.12", true, "2024-05-21"),
      P("nunomaduro/collision", "^8.1", "8.5.0", true, "2024-10-15"),
      P("phpunit/phpunit", "^10.5", "10.5.38", true, "2024-11-08"),
      P("pestphp/pest", "^2.36", "2.36.0", true, "2024-10-16"),
      P("larastan/larastan", "^2.9", "2.9.12", true, "2024-12-04"),
    ],
    locked: [],
    scripts: [
      ["post-update-cmd", "@php artisan vendor:publish --tag=laravel-assets"],
      ["test", "@php artisan test"],
    ],
  };
  composer.locked = composer.packages.map((p) => [p.name, p.version]);
  const N = (name, constraint, version, dev) => P(name, constraint, version, dev);
  const npm = {
    id: "npm",
    label: "npm",
    manager: "pnpm 9.15.0",
    lock: "pnpm-lock.yaml",
    facts: [["Node", ">=20"]],
    packages: [
      N("axios", "^1.7.4", "1.7.9", false),
      N("alpinejs", "^3.14.0", "3.14.8", false),
      N("vite", "^6.0.0", "6.0.6", true),
      N("laravel-vite-plugin", "^1.0", "1.1.1", true),
      N("tailwindcss", "^3.4.13", "3.4.17", true),
      N("postcss", "^8.4.47", "8.4.49", true),
      N("autoprefixer", "^10.4.20", "10.4.20", true),
      N("@tailwindcss/forms", "^0.5.9", "0.5.10", true),
    ],
    locked: [],
    scripts: [
      ["dev", "vite"],
      ["build", "vite build"],
    ],
  };
  npm.locked = npm.packages.map((p) => [p.name, p.version]);
  const pk = {
    highlights: [
      ["Laravel", "11.36.1"],
      ["Livewire", "3.5.18"],
      ["Tailwind", "3.4.17"],
      ["Vite", "6.0.6"],
    ],
    ecosystems: [composer, npm],
  };
  const updates = [
    { ecosystem: "composer", name: "laravel/framework", latest: "12.4.0", level: "major" },
    { ecosystem: "composer", name: "pestphp/pest", latest: "3.7.4", level: "major" },
    { ecosystem: "composer", name: "stripe/stripe-php", latest: "15.13.0", level: "minor" },
    { ecosystem: "composer", name: "phpunit/phpunit", latest: "10.5.40", level: "patch" },
    { ecosystem: "npm", name: "tailwindcss", latest: "4.0.9", level: "major" },
    { ecosystem: "npm", name: "vite", latest: "6.2.0", level: "minor" },
    { ecosystem: "npm", name: "axios", latest: "1.8.1", level: "minor" },
  ];
  window.mockLogSources = sources;
  window.mockLogPage = page;
  window.mockPackages = pk;
  window.mockUpdates = updates;
}
// The Processes window: the web server, two apps and two services.
const actRow = (key, name, state, o = {}) => ({
  key,
  name,
  pid: null,
  port: null,
  cpu: null,
  memory: null,
  state,
  detail: null,
  actions: true,
  web: null,
  ...o,
});
window.mockActivity = {
  web: [
    actRow("web:caddy", "Caddy", "running", { pid: 402, port: 443, cpu: 0.3, memory: 18350080, actions: false, detail: "2.10.2" }),
    actRow("web:php-8.4", "PHP-FPM", "running", { pid: 412, cpu: 0.1, memory: 31457280, actions: false, detail: "8.4.13" }),
    actRow("web:dns", "DNS", "running", { pid: 420, port: 53, cpu: 0, memory: 6291456, actions: false, detail: ".test" }),
  ],
  apps: [
    {
      site: "shop",
      rows: [
        actRow("shop@queue", "queue", "running", { pid: 9121, cpu: 0.8, memory: 61865984 }),
        actRow("shop@vite", "vite", "running", { pid: 9133, port: 5174, cpu: 2.4, memory: 142606336, web: "https://shop.test" }),
        actRow("shop@logs", "logs", "crashed", { detail: "exited with code 1" }),
      ],
    },
    {
      site: "vuefinder",
      rows: [
        actRow("vuefinder", "dev", "starting", { pid: 9302, port: 5173, cpu: 14.1, memory: 98566144 }),
        actRow("vuefinder@docs-dev", "docs:dev", "running", { pid: 9310, port: 5180, cpu: 0.1, memory: 75497472 }),
      ],
    },
  ],
  services: [
    actRow("service:mysql", "MySQL", "running", { pid: 801, port: 3306, cpu: 1.2, memory: 412090368, detail: "8.4.2" }),
    actRow("service:cache", "Redis", "running", { pid: 822, port: 6379, cpu: 0.2, memory: 9437184, detail: "7.4.1" }),
  ],
};
// `docker compose logs --timestamps`: each line after its time, in UTC.
const T = (s, text) => `2026-10-17T10:00:${String(s).padStart(2, "0")}.${String(s * 7919).padStart(9, "0")}Z ${text}`;
window.mockContainerLogs = {
  web: [
    T(1, "/docker-entrypoint.sh: Configuration complete; ready for start up"),
    T(4, "\x1b[32mlistening\x1b[39m on http://localhost:8080"),
    T(9, '172.18.0.1 - - [17/Oct/2026:10:00:09 +0000] "GET / HTTP/1.1" 200 615'),
    T(12, '172.18.0.1 - - [17/Oct/2026:10:00:12 +0000] "GET /favicon.ico HTTP/1.1" 404 153'),
  ],
  db: [
    T(0, "PostgreSQL init process complete; ready for start up."),
    T(2, "LOG:  starting PostgreSQL 17.2 on aarch64-unknown-linux-musl"),
    T(3, 'LOG:  listening on IPv4 address "0.0.0.0", port 5432'),
    T(5, "LOG:  database system is ready to accept connections"),
  ],
  cache: [T(6, "* Ready to accept connections tcp"), T(10, "* Background saving started by pid 21")],
  worker: [T(7, "Starting queue worker"), T(8, "\x1b[31mError:\x1b[39m connect ECONNREFUSED 172.18.0.3:6379"), T(8, "exited with code 1")],
};
// What `create::options` offers per template (the New application sheet's Options).
const mockOpt = (key, label, choices) => ({ key, label, choices, default: choices[0][0] });
const mockOnOff = (key, label) =>
  mockOpt(key, label, [
    ["on", label],
    ["off", ""],
  ]);
const mockJs = mockOpt("js", "Packages", [
  ["npm", "npm"],
  ["pnpm", "pnpm"],
  ["bun", "bun"],
]);
const mockCreateOptions = (kind) =>
  ({
    laravel: [
      mockOpt("kit", "Starter kit", [
        ["none", "No starter kit"],
        ["react", "React"],
        ["vue", "Vue"],
        ["svelte", "Svelte"],
        ["livewire", "Livewire"],
      ]),
      mockOpt("db", "Database", [
        ["sqlite", "SQLite"],
        ["service:mysql", "MySQL"],
      ]),
      mockJs,
      mockOpt("php", "PHP", [
        ["8.4", "PHP 8.4"],
        ["8.3", "PHP 8.3"],
      ]),
    ],
    next: [
      mockOnOff("ts", "TypeScript"),
      mockOnOff("tailwind", "Tailwind"),
      mockJs,
      mockOpt("node", "Node.js", [
        ["24", "Node 24"],
        ["22", "Node 22"],
      ]),
    ],
  })[kind]?.concat(mockOnOff("git", "Git"), mockHttps()) ?? [mockOnOff("git", "Git"), mockHttps()];
// Its default follows Settings › HTTPS for new applications.
const mockHttps = () => ({ ...mockOnOff("https", "HTTPS"), default: window.mockHttpsDefault === false ? "off" : "on" });
const mockHandle = (cmd, a) => {
  const ov = a?.site && mockOv[a.site];
  switch (cmd) {
    case "state":
    case "app_run":
    case "worktree_adopt":
    case "worktree_copy_files":
    case "worktree_after_create":
    case "worktree_install":
    case "leftover_drop":
    case "leftover_keep":
      return mockSnap;
    case "worktree_add":
      if (window.mockAddFail) throw window.mockAddFail;
      if (!mockSnap.sites.some((x) => x.worktree_of === a.site && x.branch === a.branch))
        mockSnap.sites.push(
          mockSite(`${a.site}--${a.branch.replace(/[^a-z0-9.-]/g, "-")}`, "laravel", { worktree_of: a.site, branch: a.branch }),
        );
      return mockSnap;
    case "worktree_after_create_list":
      if (window.mockAfterListFail) throw window.mockAfterListFail;
      return window.mockAfter ?? [];
    case "worktree_rewrite":
      if (window.mockRewriteFail) throw window.mockRewriteFail;
      return mockSnap;
    case "add_alias":
    case "set_group":
      if (window.mockAskFail) throw window.mockAskFail;
      return mockSnap;
    case "worktree_hooks":
    case "set_app_hooks":
      if (window.mockHooksFail && cmd === "set_app_hooks") throw window.mockHooksFail;
      return {
        hooks: {
          copy: [".env"],
          copy_database: false,
          keys: { APP_URL: "{url}", DB_DATABASE: "{db}" },
          check: ["DB_DATABASE"],
          after_create: ["composer install"],
          from: {
            copy: "Laravel preset",
            copy_database: "Settings",
            keys: "this app",
            check: "Laravel preset",
            after_create: ".anvil.toml",
          },
          preset: "Laravel",
          problems: window.mockProblems ?? [],
        },
        own: { keys: { APP_URL: "{url}", DB_DATABASE: "{db}" } },
      };
    case "worktree_remove":
      if (window.mockRemoveFail && !a.force) throw window.mockRemoveFail;
      return mockSnap;
    case "worktree_new_db":
      return window.mockNewDb ?? { can_copy: true, copy: false, main_database: "shop" };
    case "worktree_prepare":
      if (window.mockPrepareFail) throw window.mockPrepareFail;
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
      return ov;
    case "containers_run":
    case "container_run": {
      for (const c of ov.containers ?? [])
        if (!a.service || c.service === a.service)
          Object.assign(c, a.action === "stop" ? { state: "exited", health: null, exit_code: 0 } : { state: "running", exit_code: null });
      return ov;
    }
    case "container_log_read": {
      // The stream's buffer: lines after number `after`.
      const all = window.mockContainerLogs[a.service] ?? [];
      return {
        text: all
          .slice(a.after)
          .map((l) => l + "\n")
          .join(""),
        next: all.length,
      };
    }
    case "container_logs_stop":
      return null;
    case "command_run": {
      const c = ov.commands.find((x) => x.id === a.id);
      Object.assign(c, { running: true, exit: null });
      return ov;
    }
    case "process_add": {
      if (ov.rows.some((x) => x.id === a.name)) throw `${a.name} is already a process of this app`;
      ov.rows.push(mockRow({ id: a.name, name: a.name, command: a.command, source: "Anvil", added: true }));
      return ov;
    }
    case "processes_reset":
      ov.rows = ov.rows.filter((x) => !x.added).map((x) => ({ ...x, manual: false, autostart: false, edited: false }));
      ov.hidden = [];
      return ov;
    case "log_clear":
      // Like the backend, which empties the file: its next read finds nothing.
      (window.mockCleared ??= new Set()).add(`${a.site}/${a.source}`);
      return null;
    case "set_process": {
      if (a.command === "") throw "The command can't be empty";
      const r = ov.rows.find((x) => x.id === a.id);
      if (a.host != null) {
        if (a.host === "docs-site") throw "vuefinder--docs-site.test is already the address of docs:site";
        Object.assign(r, { address: `https://vuefinder--${a.host}.test`, web: `https://vuefinder--${a.host}.test`, host_edited: true });
      }
      if (a.command != null) Object.assign(r, { command: a.command, edited: true });
      if (a.autostart != null) r.autostart = a.autostart;
      if (a.manual != null) r.manual = a.manual;
      if (a.top != null) {
        r.top = a.top;
        ov.rows.sort((x, y) => y.main - x.main || !!y.top - !!x.top);
      }
      if (a.hidden === true) {
        ov.rows = ov.rows.filter((x) => x !== r);
        if (!r.added) ov.hidden.push({ id: r.id, name: r.name });
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
      const text = window.mockCleared?.has(`${a.site}/${a.source}`)
        ? ""
        : (window.mockLogs?.[a.source] ?? mockSiteLogs[a.site]?.[a.source] ?? (a.source === "once:build" ? mockBuild : mockLog));
      return { text: text.slice(a.from > text.length ? 0 : a.from), end_offset: text.length };
    }
    case "log_sources":
      return window.mockLogSources;
    case "log_read":
    case "log_search":
      return window.mockLogPage;
    case "log_tail":
      return { entries: window.mockTail?.splice(0) ?? [], start_offset: 0, end_offset: window.mockLogPage.end_offset, truncated: false };
    case "packages":
      return window.mockPackages;
    case "package_updates":
      return window.mockUpdates;
    case "package_audit":
      if (a.ecosystem === "npm") throw "pnpm audit failed: request to registry timed out";
      return [
        {
          package: "guzzlehttp/guzzle",
          version: "7.9.2",
          severity: "high",
          title: "Noncanonical host can bypass host-based checks",
          url: "https://github.com/advisories/GHSA-v5mv-p594-2x33",
          fixed: null,
        },
      ];
    case "activity":
      return window.mockActivity;
    case "activity_log": {
      const text = mockActivityLogs[a.key] ?? "ready in 412 ms\n";
      return { text: a.from ? "" : text, end_offset: text.length };
    }
    case "services":
    case "service_run":
      return mockServices;
    case "service_contents":
      return {
        databases: [
          ["shop", 52428800],
          ["blog", 1048576],
          ["mysql", 2621440],
          ["sys", 40960],
        ],
        keys: [["db0", 12]],
        memory: "911.12K",
        messages: 3,
      };
    case "once_stop":
      Object.assign(
        ov.commands.find((c) => c.id === a.id),
        { running: false, exit: null },
      );
      return null;
    case "link_folder":
      return "notes";
    case "create_app":
      if (window.mockCreateFail) throw window.mockCreateFail;
      return "notes";
    case "create_log": {
      const log = window.mockCreateLog ?? "";
      return { text: log.slice(a.from), end_offset: log.length };
    }
    case "create_options":
      return mockCreateOptions(a.kind);
    case "create_reveal":
      return null;
    case "touched":
      return Math.floor(Date.now() / 1000) - 7200;
    case "service_kinds": {
      // As services::kinds: each version's license and when it was accepted.
      const lic = (kind, v) =>
        kind === "redis"
          ? v.startsWith("7.2")
            ? { id: "redis:BSD-3-Clause", name: "BSD-3-Clause", url: "https://github.com/redis/redis/blob/7.2/COPYING" }
            : { id: "redis:RSALv2-SSPLv1-AGPLv3", name: "RSALv2, SSPLv1 or AGPLv3, your choice", url: "https://redis.io/legal/licenses/" }
          : {
              id: "mysql:GPL-2.0-only",
              name: "GPL-2.0 with the Universal FOSS Exception",
              url: "https://github.com/mysql/mysql-server/blob/trunk/LICENSE",
            };
      const kind = (k, name, category, versions, port, accepted = {}) => ({
        kind: k,
        name,
        category,
        installed: false,
        versions,
        default_version: versions[0],
        installed_versions: [],
        suggested_name: k,
        suggested_port: port,
        default_port: port,
        licenses: versions.map((v) => [v, lic(k, v), accepted[v] ?? null]),
      });
      return [
        kind("mysql", "MySQL", "Databases", ["9.7.2", "8.4.11"], 3306),
        kind("redis", "Redis", "Cache & Queue", ["8.10.2", "7.2.16"], 6379, window.mockRedisAccepted ?? {}),
      ];
    }
    case "git_info":
      if (a.site === "notes") throw "Not a git repository"; // a plain folder
      return {
        branch: "main",
        detached: false,
        ahead: 0,
        behind: 0,
        changed: window.mockGitClean ? 0 : 3,
        upstream: null,
        commit: null,
        remote: null,
      };
    case "unhide_site": {
      const h = mockSnap.hidden.find((x) => x.name === a.site);
      mockSnap.hidden = mockSnap.hidden.filter((x) => x !== h);
      mockSnap.sites.push(mockSite(h.name, h.kind));
      return mockSnap;
    }
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
        php: ["8.5", "8.4", "8.3", "8.2", "8.1", "8.0"].map((version) => ({
          version,
          installed: version === "8.5" || version === "8.4",
          default: version === "8.4",
          pinned_by: window.mockPhpPins?.[version] ?? [],
        })),
        ini: { memory_limit: "512M", upload_max_filesize: "64M" },
        terminal: "Terminal",
        terminals: ["Terminal", "Ghostty"],
        git_client: null,
        git_clients: ["Fork"],
        theme: null,
        dock_icon: true,
        trim_app_logs: true,
        notifications: true,
        https_default: true,
        worktree_db: "copy",
        worktree_presets: [
          {
            id: "laravel",
            name: "Laravel",
            kinds: ["Laravel"],
            hooks: { copy: [".env"], keys: { APP_URL: "{url}" }, check: [], after_create: ["composer install"] },
            edited: true,
          },
          { id: "other", name: "Other", kinds: [], hooks: { copy: [".env"], keys: {}, check: [], after_create: [] }, edited: false },
        ],
        login: false,
        config_dir: "/Users/x/Library/Application Support/Anvil",
      };
    case "storage_anvil": {
      // As resources::anvil_disk: largest first, total is their sum.
      const A = "/Users/x/.anvil";
      const item = (label, path, bytes, category, safety, note) => ({ label, path: `${A}/${path}`, bytes, category, safety, note });
      const items = [
        item("mysql data", "data/mysql", 1932735283, "Data", "keep", "Your databases and mail"),
        item("PHP 8.4", "php/8.4", 412090368, "PHP", "keep", "Remove versions in Settings › PHP"),
        item("PHP 8.3", "php/8.3", 398458880, "PHP", "keep", "Remove versions in Settings › PHP"),
        item("MySQL", "services/mysql", 318767104, "Services", "keep", "The program, shared by every service of this kind"),
        item("Node.js", "runtimes/node", 196083712, "Languages", "keep", "Remove versions in Settings › Languages"),
        item("composer cache", "cache/composer", 88080384, "Cache", "safe", "Downloaded again when a build needs it"),
        item("Web server", "bin", 47185920, "Anvil", "keep", "Needed to serve your apps"),
        item("Logs", "log", 12582912, "Logs", "safe", "Only history; new lines keep coming"),
        item("redis data", "data/redis", 2097152, "Data", "keep", "Your databases and mail"),
        item("Certificates", "data/caddy", 65536, "Anvil", "keep", "Your local HTTPS certificates"),
      ];
      return { total: items.reduce((s, i) => s + i.bytes, 0), items };
    }
    case "runtimes":
      return [
        { id: "node", name: "Node.js", installed: ["24", "22"], available: ["26", "24", "22", "20"], pinned_by: { 22: ["blog", "shop"] } },
        { id: "go", name: "Go", installed: [], available: ["1.27.2", "1.26.8"] },
        { id: "rust", name: "Rust", installed: ["stable"], available: ["stable"] },
        { id: "dotnet", name: ".NET", installed: ["10.0"], available: ["10.0", "9.0", "8.0"] },
        { id: "python", name: "Python", installed: ["3.13"], available: ["3.14", "3.13", "3.12", "3.11"] },
      ];
    case "package_managers":
      // As runtimes::package_managers: for apps, and for the terminal when it differs.
      return [
        {
          name: "npm",
          apps: { version: "11.6.2", path: "/Users/x/.anvil/runtimes/node/24/bin/npm", source: "Bundled with Node.js 24" },
          terminal: { version: "11.12.1", path: "/opt/homebrew/bin/npm", source: "Homebrew (Node.js 26)" },
          same: false,
          anvil: false,
        },
        {
          name: "pnpm",
          apps: { version: "12.14.0", path: "/opt/homebrew/bin/pnpm", source: "Homebrew" },
          terminal: null,
          same: true,
          anvil: false,
        },
        { name: "yarn", apps: null, terminal: null, same: false, anvil: false },
        {
          name: "bun",
          apps: { version: "1.3.2", path: "/Users/x/.anvil/runtimes/bun/bin/bun", source: "Anvil" },
          terminal: { version: "1.3.1", path: "/Users/x/.local/share/vite-plus/bin/bun", source: "Vite+" },
          same: false,
          anvil: true,
        },
      ];
    case "package_manager_install":
    case "package_manager_uninstall":
      if (window.mockPmError) throw window.mockPmError;
      return null;
    case "storage_last":
      return window.mockStorageLast ?? null;
    case "clean_all_safe":
      return [162529280, []];
    case "agent_activity":
      return (window.mockAgents ?? []).filter((x) => !a.site || x.site === a.site);
    case "update_check":
      return window.mockUpdate ?? null;
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
