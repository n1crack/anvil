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
    case "log_text":
      return { text: a.from ? "" : mockLog, end_offset: mockLog.length };
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
    case "app_icon":
      return window.mockNoIcons
        ? null
        : ({
            Finder:
              "data:image/png;base64,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",
            Zed: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAAXNSR0IArs4c6QAAAHhlWElmTU0AKgAAAAgABAEaAAUAAAABAAAAPgEbAAUAAAABAAAARgEoAAMAAAABAAIAAIdpAAQAAAABAAAATgAAAAAAAACQAAAAAQAAAJAAAAABAAOgAQADAAAAAQABAACgAgAEAAAAAQAAAECgAwAEAAAAAQAAAEAAAAAAlNz6EQAAAAlwSFlzAAAWJQAAFiUBSVIk8AAAEuVJREFUeAHtm1usXdV1hue+nLsvyGCwDU1twHZSkYbymIangASEm502qtRWbZ/SVJTiYNM2QEJVKkB9oAYCvFatIDaRCMI2UiKQSagKaaBOTInzQiqgiYltiDA+x+ecfen//WPOtfexzzpQkrd6+qy15hxzzHH5x5iXvfZ2SmfKGQTOIHAGgTMI/P9FoPGruL7+0kvPmmo01nXn589tNZvLJWtMV7uVWo1uvyPZrdRspUaz32x0u91Gv9lviK+Rein1m8m6e92UmhrUaLX6KXVTd77XbzQaqqfq2Wq1Uq/X60tGvznS7Kp/ttlvvdfvN470emM/O3jwhXfF/5HK/xmAzZs3L2+021ubjdbnG430O/1+/7zUaIwgSIZlIxpJ9IThPLma9PHXaLre7fVE73lMQx0N4dIXTX5aRqMpWDROtxjTaqZeV2hZiMnq6c83m80jrXbrgGB8sjs5/s0D+/f/0gI+5K1Y/KHYN378kq2y/+/lzG/hEI5VPsvWqi5pOAIgBBw/KkXGoWrhhB0FGJwdLoyH07IkB0CVBZYLn/sFlEAQgE6pnwjTu37wg5e+MSxnqXprqc6hvsbGjZvvVpQelI3nYqhSUt0YH84RQUqvG3TMARCiSo9g0D0cJPKgAr2lyEJ3hqgGYNANbh7DWJx1xsAjhnBaoAg4S5Y80c5R9++tWbtu2Wc+87vPvfbaa6FQxLryoQDAeVlwu5yRdYqZopv9FTmyoHKScKvkaWzHcA4woOGunQaEPBYA6Md5HHLKiGYnLStAKXJ4BlAI5Q/5AgiwEZQanz5y7J2ptw///NtqLFk+EIBNF23aIvkP9fo9ZRlRMN4RBZRLoVUSB8/pHCHo4id6/LNdGluiSDvGwU8t2kgHg1bWZUdzvxZQTwPzApbarutmLRrMdCE7BcanV51z9k+OHjnyqplqbqG5pvPiiy9eIRX/IYs2CYCMbnZaIyMSGCEH+ZcNRRwG4dKAhCpoOKgxBrL0l7UCgHLkNRBu+elhyLO+nPKOeJbBukmVsYzpahoCgq7/breblx08eLB2lwgINWixIiWfl7GbeuxVEh7zHgCIEItPWBeOSzXNfEELejgCHZgwEAHa9vxkxcBL92Wntf9pDww+0oF2U/wAB92ZhQyApK060pFtEJheKuJbPzfX+QM3am5LAqA0+kKYFkqKImSFuqJcBBsRDhIRTMBYjHcb6+CRQ07TzCOC+zuKWBfGhrZO07IsDSvOM5YLxwHQoElJj+DkACGbLbbb7ThgmnK/j2Zdi5ZaAC644IJVmkiXsu3gLFPA819O4AsX9pao2mho8hx6OKp0VCN4Gq7T5iogdJSuOt8woJIFTf64jSy6kdHQqQq5OgB5vAbZcUbDH2nf9U5kIDod0Xuf3LBhg3auxUt7cXJKIyMjv6GIs61IufTmCMuMqk5E6XOoM4+jjpOymqhQgJ/gRQ1jBYr6EYljWA9w5jZzOFhtperA6W6RB49pHuqI0zYAyPYV64A4V8n2j+n5tq7TSi0Awvo8DWzbCQTKWrYlz08pY5WmeLqpCuKdudk0Py/U/U9ExokH8LgYgpGAWIp3FssRp3VkXulCH+lfCrwtZcHo6Ij0Ijn0UidDsdUHJdFDMxz9ZqfTWUNtsVILgE7lKxqEk+LwKdpSUJzBMxSaQ/W5ubm0YcOF6brrrk0rVqzwsGF+6qUYiNyUzwak4lVn1GN2xkILlnJJ1wsv/Fv69xdfNBDIY4HudmKa0h+0OGYDoKU3GmGQexfeagGQo2O4V7a/EE4UiRALGdtiID0/N5/Wr1+f/vVf/jmtXbt2oYZfc+uKK65IN9ywJZ2cnTX4HTkvZPTHsquq7AqwArSsfjw/T3vUAqCUbjeasRjhJt6CL3tsoxHKmhB0Ef3rr7vOzt999z+k55//bhpRmlKcMa4NTRuNceoDoiM+xCc9JcsMtJwbGRlNd9x5e/rkJZek77/0/TQ9PW2JOEoG8CQTCUo4n8GwHk+lwTzKtpRHLQBadNo43TC6sOMtzksWVd1YD1gE+jJixXI+Dae0f//+9Oqrr6bRsTFnS8kUWOPEJ5nio42j9Huus75Ilj8UiR59fc33sXTPvffY+X3PPJPu/OrXFIQOEnK0pR/FBgPH3YJiGQGIPnzXlCUA6LeRhTgjPLQOSLIVopSrLELoGGm3U5tLH3LmtQ1NjI8rgm0fnJCGY03LWmhozNcwGjmIbivyX7vrrnT11VenZ+T8X9/2N842+gmGNHsHIDfDFFtrHeYgw6SPhbGuLAFAt9HrFeBiYbKzRFLRIopue+4NI48RPRt61VVXpW3btqWVK1cu0M9YuMDRFXot0DeJ7Ws3mUvj4xOeVvv27Uu32XnNe+n22QTgw2vcR4Lakfpu6FZALe3FnrUAaLj5Yy8PhElPR0+KocMRNoQBRQG08fGxtH379qTPE+nQoUOlK57ZcQMQnlf9ODeurNm0aZNp+/buSztu25FmZ+fskJ1XZItd+B4ZmkVLaLRz0CrJi1eWACAE42He7Ix0zNkSuniyU+Qg2DBGtLUILlu2LP340I/Tlq1bjVRsaRrjP+Z5ziZVkIRzOoCle+65zwAQ+R07dnjFJ5pe8bMi+Dl7xLqhFjHAVtCnT0/SX2+L3K67LQFALEicNzDU5w6Eq+Hoo1OORxqiPxSjCDbWhTAmTnUYA5AdDkpiwCFGVPu8ojo1NZXuvfe+dM0116Ti/Kx2mKYOP3G212s2HNJ42sVZ2wGEooeeIVtseJ37WmfqumRP6vHyQ0L1F1GT0dJS/sIA92cGejP/sF5nkFg6nV5ar8PS+evW2lDAlNkGFYf++A//KLFu7FXab9+xPc2enK3SPnaThk+BnDvIlhiKBGyMBQ/9BgGTVBjHVVdqAYgBIdjey1S/ccHksNpbJHzoKtEgO1Cohwt0Pp3xQvOiCy9M39y9K5199tnRucgd52/V2jHHQYcssby81wuwsdFRO4+OUxe5cjzWQlVJLnZVhFMqtQAYOdK28sQ4G12RCZ4WQfpjGlQq5XgYDeqi0pax8/Pzad26dXaes8L3dKTl1djExGT6sz/9k3TWWWelPXv22vlZOc9i29bnALbU48ePWwbnCBZIZBUdRBu+bj8yQiEj7LazrAOnAjWMQdnnhmm5ro+VEjSYy4SUjCgX60IsfoXGwDBMPIzN/yIjYt5Sf/6730v33ndf+vrDj6Tzz1+Xnd+Ttn35y+nkzAxCpDt2Axym7dVf8sZ0wMrfE2R7eqnDehCLlHkLOA6ibNSHIUxbtNRmAFOsqV6EUfwQ2pxhTINAgPU0SDIu+MJ5N/INMIjSj374o/S5a69Nh39+OJ137uq08592+sPTnj170i23bNNWNztwTnInJyc8jukT00/bqwBoe2WXnjCqsqcsqJ6iNkc3/ZX1YtimUq8FgM3DzvHM6HJu5ysdhBJdrZFMgioTEMqUoLe8RlPV45tK9/ffP55efvnlNDkxkR588EE7//TTT8v5W8yzfPmyNDM9Y2eRMyW+lqZATLNI6zFlBNOCLZBp6DfLKFadtcbprj7S32R4lii1U4AZFanLHM9OSSjv/UsGAMJg0QuF9PkDioyJAt0oeCGcmpxMDzzwgJy/Lu3WgvjnX/xievedd9L69b+ZrvjsZ/VRerkiFlvcpHinpiZjvO44zJRotwCFDGAKok919GX7sJu67Qwjau+1GSCpCnaswtmF8ENGGDWQRQnKeRYVpuF838YyZzG0qy1wbGw87dy5M11//fXpyJEj6cB/HtCU+FyaPjGd1qxZ42MvqzwfrljJcb7d1ssPyQRUFjw+W5Ss8NcUJcK2ByNYe0iI6PggEOoBCHcXOod8Fauwx97hw3kZSQEKpgor9R133K6nXk5qDnM0Ju1xnrJq1ar0t1/5Sjp69KiuI+nY0WPp9Z++nqZnpq0ToZM6GJHuhL6s6BOaFoMpwDQIz7N6W82N12esCUzRpUotACRRo4qulKAhQI1qVY8MKH3uFGtPabzrG7ts+MqVK+T8Q3Z+79496bHHHrPhvMnhXQIffFioDh8+LCCOug91E5oCI1W644jOAWSAFtSSdaR7gEBv8BRTyU6C5SlRg0I9AJpY5dsZxDrJWQC98glZiCpeBJkuuR0P3RWZtj4Gk/YPPfR1vcW5IT311FPp5ptvTidOvO9DThnkGEoABx/2+pK2pHtbnw3sLP3qYw0oAJRxYYlzT/YMpoDlFMOC6bR7LQBaUwUg797lTOVcdg9jcB5j9Y9FiPcAlHjpodHae0fl/MMPP5xuvPHG9OSTT6a/lPNMDegeq7UhdhgSDBAlS1cxHD5egBpmYxoAxBSIT4TDIIQ1WBGR9wSQvI+0DbL1tvnNQjYI13Gb19P2nrb6MACDXnzppTS1bCq9++67dmJci1lxXixp48aN6YknnvCYeaX9nIDguMvezwce6jEd5g3SrN4wrz5ndXrjjTf8YsWgSM6o5DoDEIptA3OgQIoiut9nioElua7UZoAilANBRLLzgoBaKBlo5vXX7id2p8cff8wfZyd1vH3kkUfTli1b0ltvvaWUn47tS+ncwXF9mMHZWb3uGh2dTaNydk4yDIDXhPk00ZlIx44dS9/5zre9RpAxTKtyEmTbwxpPRXVRMLj4b6+h6/K7S3OcfqsFIOThLBeRD4UWkVPfW40UwDOi7YrvC8fk1COPhvOk/a236oONHGZbwzqjGsKBEkJFo04pdI6wZAvbHlMKf0oGBKP4AcbDYmyphyjs/oiLoCaOPnxzDgiTrEV1Ui6U2AQbVV6TE50S+d27d6e/+NKXlN7zQyk7GBpOFtkhq+rNQEBFHVnDOYC5zJrAO8YAEoP0Jyb4KDy1JEUNOQBUCaa6sNRmgI4ifZYAxiKHKkormUWjVPpEJo77778/bdXbn1deeSU9qizQ74kcPZ/UPDYWTJwvByj99MnCQ3YshO6zXhRDCxA2f+LjOhwt03oxFw7TzRyIv/AMZmz1Leom1NxqAdC7f50lIoWyzCLWokh/MHAGSg/H1ssvv9x9l112WXr22efMT9RI5eGLnYAL2mDhgyfTc5/H6DxhGf7KLaU333wj/fDAAS28+rq8CoLVhtNolT2ATMksQnnxUguAjplzTaFrYRWqrPoDrcUATlzTJ06km266KV155ZXm8XamhcovQ/TkNMiBJL665htcpXSmL3gq1fm8QcrDXy7cOX78PX8x8oujvxAA8cWL3axuCpj9DLuLy8qu2VI/9Tnw5pQeRfRqbTf7nN6VApgEgTznuBvPOLyw/8/pREfUzKN7mTKOR1gG1aCqElAaxarTMumDL0qY6Lt42XLjC9NMLx4oSMVM60WPQ2E7tuilyreywAWP2gwQ11HtozoN6NeOITocZnhYUypQzDEyMqYFit9KxvmgSkWMq7KI7uwccio/1Sh0kSkDF6LtuwErfQwJBGJo6KEeYpGgb+JTOjIkYUF1KQD+R6PfEffqagRSS+Sl1xlgNEi9MIQ7bMMGMT4MGtRK26N0G/Y9JMFbUpq6SnY+GllmHljG8yxgZ75f9judt8qYU5+17wP0BeTbmn//VQz1wIFlbtJXXdKM8jjK6qmeqk4fvO6nL4+jrXpsW3CgIE5uFU/hBQyx+BqqM7Ycn2PKISf4eErOoZMnT/6M+mKlFgAxaylKe8LUU4cOIo66YUepu63BNk79xXBAgd8m6lboPIsjRZb5fCt8ABwgxxY6LEh1ZJd+BIYWCe7tVUMnscXLUgAkLZ27JUfpkxc8pSD/qiI9RWkYLiNw3M4vNBYHw66IpFjcxlSKTc488JWrjCttSc3cgFacjmf4nAfD1e+TxY8jv64s/b1Rp/Oe3rHxg+RrmH7VnPdchCCx+axQvKkMtB0YG86V/iBkuh7lYzUOlmKIc7uQTYMBgq+oMM5mhPfqg64iG+X83yn9yYDasjQAGqZDyEF+MCUQPoWUKgNsEdEMl0MDkVAtbkEqd+jDnkH3Cp4NdjvzFJL4PaSQRadtUbnCZKSUSRmBIgv7u2ZmZu5UV236M+4DARCPjuLzLwiEdQLhtxlUSnF9EEWMCYMKz+nP0l9cg+NUWrQrZ3E7/iyuOD2QXcbDpt5ef/f0zMxfqb/2F6Jl7IcBAN5pgfCcfjPznt7afEKHoPg5SE43HgYj28Fj4B4tXcWbytkhOhqgIyif7UlhhhQ5yHe9EMoYP8XnaZne1pck/zhzcvarIh/NXUs+FohbkjM6W8qET422218QEJx5L5Jd/vUDU8HoVw4OSUML/i6hrZpa1fg8KBwLYchQP86iz/L6Sf9zJL2uD1XP6svXXQrUK2KqPfuHoMF9CZMGTIvUOECdq2Ppek2Lj8mg1Xp9skKvYUdFJ6s+kly2JHaHU0vZqtQHBHKuN6cT4HGBoP8y03tTx++fis4PIeu/AztVaG5/JEOXkPXrlFejxmSA4DpTziBwBoEzCJxB4FdB4H8B1LofV+WLvU0AAAAASUVORK5CYII=",
          }[a.name] ?? null);
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
