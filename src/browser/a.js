// Anvil browser bridge: sends this page's errors to /__anvil/e on its own origin, where
// Anvil's web server writes them to the app's browser log for AI agents (`anvil mcp`).
// Collects JS errors, unhandled rejections, console.error/warn, failed fetch/XHR, failed
// resource loads, CSP violations, deprecations/interventions and the page load. Never
// cookies, storage, request or response bodies, form values or headers. Everything the
// page calls through it gets what it gave and returns what it returned; a failure in
// here is swallowed.
(() => {
  if (window.__anvil) return;
  Object.defineProperty(window, "__anvil", { value: 1 });
  const E = "/__anvil/e";
  const BEACON = 6000; // encoded bytes per beacon; Caddy refuses URLs past ~16 KB
  const q = [];
  const seen = new Map();
  const page = Math.random().toString(36).slice(2, 10);
  let timer = 0;
  let kept = 0;
  const cut = (s, n = 1500) => (s == null ? "" : String(s)).slice(0, n);
  const fmt = (a) => {
    if (a instanceof Error) return a.stack || String(a);
    if (a && typeof a === "object") {
      try {
        return JSON.stringify(a);
      } catch {
        return Object.prototype.toString.call(a);
      }
    }
    return String(a);
  };
  const safe = (f) =>
    function () {
      try {
        return f.apply(this, arguments);
      } catch {}
    };
  const flush = safe(() => {
    timer = 0;
    while (q.length) {
      const batch = [];
      let len = 0;
      while (q.length) {
        const n = encodeURIComponent(JSON.stringify(q[0])).length + 3;
        if (batch.length && len + n > BEACON) break;
        batch.push(q.shift());
        len += n;
      }
      navigator.sendBeacon(E + "?p=" + page + "&d=" + encodeURIComponent(JSON.stringify(batch)));
    }
  });
  // At most 200 events a page; a repeat is counted, and said at its 2nd, 10th and 100th time.
  const add = safe((o) => {
    const key = o.k + "\n" + (o.level || "") + "\n" + (o.msg || "") + "\n" + (o.url || "") + "\n" + (o.line || "");
    const n = (seen.get(key) || 0) + 1;
    seen.set(key, n);
    if (n > 1) {
      if (n === 2 || n === 10 || n === 100) q.push({ k: "repeat", of: o.k, level: o.level, msg: cut(o.msg, 200), url: o.url, line: o.line, n });
    } else if (kept++ < 200) q.push(Object.assign({ t: Date.now(), at: cut(location.pathname + location.search, 500) }, o));
    if (!timer && q.length) timer = setTimeout(flush, 1000);
  });
  // Where console.error was called from, a.js's own frames dropped when read.
  const here = () => {
    try {
      return cut(new Error().stack, 1500);
    } catch {
      return "";
    }
  };

  addEventListener(
    "error",
    safe((e) => {
      const el = e.target;
      if (el && el !== window && el.tagName) return add({ k: "resource", tag: el.tagName.toLowerCase(), url: cut(el.currentSrc || el.src || el.href, 500) });
      add({ k: "error", msg: cut(e.message), url: cut(e.filename, 500), line: e.lineno, col: e.colno, stack: cut(e.error && e.error.stack) });
    }),
    true,
  );
  addEventListener(
    "unhandledrejection",
    safe((e) => {
      const r = e.reason;
      add({ k: "rejection", msg: cut(r instanceof Error ? r.message : fmt(r)), stack: cut(r && r.stack) });
    }),
  );
  for (const level of ["error", "warn"]) {
    const orig = console[level];
    if (typeof orig !== "function") continue;
    console[level] = function () {
      try {
        add({ k: "console", level, msg: cut(Array.prototype.map.call(arguments, fmt).join(" ")), stack: here() });
      } catch {}
      return orig.apply(this, arguments);
    };
  }
  const of = window.fetch;
  if (typeof of === "function") {
    window.fetch = function (input, init) {
      let method = "GET";
      let url = "";
      try {
        url = cut(typeof input === "string" ? input : (input && input.url) || String(input), 500);
        method = String((init && init.method) || (input && input.method) || "GET").toUpperCase();
      } catch {}
      return of.apply(this, arguments).then(
        (r) => {
          if (r && r.status >= 400) add({ k: "fetch", method, url, status: r.status });
          return r;
        },
        (err) => {
          if (!err || err.name !== "AbortError") add({ k: "fetch", method, url, msg: cut(err && err.message) });
          throw err;
        },
      );
    };
  }
  if (window.XMLHttpRequest) {
    const X = XMLHttpRequest.prototype;
    const open = X.open;
    const send = X.send;
    const req = new WeakMap();
    X.open = function (m, u) {
      try {
        req.set(this, [String(m).toUpperCase(), cut(u, 500)]);
      } catch {}
      return open.apply(this, arguments);
    };
    X.send = function () {
      try {
        const x = this;
        x.addEventListener(
          "loadend",
          safe(() => {
            const a = req.get(x) || [];
            if (x.status === 0 || x.status >= 400) add({ k: "xhr", method: a[0], url: a[1], status: x.status });
          }),
        );
      } catch {}
      return send.apply(this, arguments);
    };
  }
  addEventListener(
    "securitypolicyviolation",
    safe((e) => add({ k: "csp", msg: cut(e.effectiveDirective + " blocked " + (e.blockedURI || "inline")), url: cut(e.sourceFile, 500), line: e.lineNumber, col: e.columnNumber })),
  );
  if (window.ReportingObserver) {
    try {
      new ReportingObserver(
        safe((rs) => rs.forEach((r) => add({ k: r.type, msg: cut(r.body && r.body.message), url: cut(r.body && r.body.sourceFile, 500), line: r.body && r.body.lineNumber }))),
        { types: ["deprecation", "intervention"], buffered: true },
      ).observe();
    } catch {}
  }
  addEventListener(
    "load",
    safe(() =>
      setTimeout(
        safe(() => {
          const n = performance.getEntriesByType("navigation")[0];
          if (n) add({ k: "load", status: n.responseStatus, type: n.type, ttfb: Math.round(n.responseStart), dom: Math.round(n.domContentLoadedEventEnd), load: Math.round(n.loadEventEnd) });
        }),
      ),
    ),
  );
  addEventListener("pagehide", flush);
})();
