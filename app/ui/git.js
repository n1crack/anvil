// Git: a button for the git app and a Git group in the Overview.

const gitCache = new Map(); // site name -> info, null when not a repository
let gitShown = null;

async function loadGit(name) {
  try {
    gitCache.set(name, await invoke("git_info", { site: name }));
  } catch (e) {
    gitCache.set(name, { error: String(e) });
  }
  if (selected === name) drawGit(name);
}

function ago(secs) {
  const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  const d = secs - Date.now() / 1000;
  for (const [unit, n] of [["year", 31536000], ["month", 2592000], ["week", 604800], ["day", 86400], ["hour", 3600], ["minute", 60]]) {
    if (Math.abs(d) >= n) return rtf.format(Math.round(d / n), unit);
  }
  return "just now";
}

function drawGit(name) {
  const root = $("#site");
  const overview = $('[data-panel="overview"]', root);
  if (!overview) return;
  const g = gitCache.get(name);
  const repo = g && !g.error;

  let btn = $(".git-btn", root);
  if (!btn) {
    btn = el("button", "git-btn");
    btn.innerHTML = `<svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="4.5" cy="3.5" r="1.5"/><circle cx="4.5" cy="12.5" r="1.5"/><circle cx="11.5" cy="5.5" r="1.5"/><path d="M4.5 5v6M11.5 7c0 2.5-3 2.5-6 4"/></svg><span></span>`;
    btn.onclick = () => open("git");
    $(".editor-btn", root).after(btn);
  }
  btn.hidden = !repo || !snap.git_client;
  btn.title = `Open in ${snap.git_client} (⌘G)`;
  $("span", btn).textContent = snap.git_client;

  $(".git", overview)?.remove();
  if (!g) return;
  const box = el("div", "git");
  if (g.error) {
    box.append(el("h3", "res-sub", "Git"), el("p", "error", g.error));
    return overview.append(box);
  }
  if (!repo) return;

  const list = el("dl", "group");
  const row = (label, ...content) => {
    const r = el("div", "row");
    const dd = el("dd");
    dd.append(...content);
    r.append(el("dt", null, label), dd);
    list.append(r);
  };

  let branch = g.detached ? `Detached at ${g.branch}` : g.branch;
  if (g.ahead) branch += ` ↑${g.ahead}`;
  if (g.behind) branch += ` ↓${g.behind}`;
  row("Branch", branch, ...(g.upstream ? [" ", el("span", "muted", g.upstream)] : []));
  row("Changes", g.changed ? `${g.changed} changed file${g.changed === 1 ? "" : "s"}` : "Clean");
  if (g.commit) {
    const c = g.commit;
    const when = el("span", "muted", ` ${c.hash}, ${c.author}, ${ago(c.time)}`);
    when.title = new Date(c.time * 1000).toLocaleString();
    row("Last commit", c.subject, when);
  }
  if (g.remote) {
    const a = el("a", "site-url", g.remote.replace("https://", ""));
    a.href = "#";
    a.onclick = (e) => {
      e.preventDefault();
      invoke("open_url", { url: g.remote }).catch((err) => showError(String(err)));
    };
    row("Remote", a);
  }
  box.append(el("h3", "res-sub", "Git"), list);
  overview.append(box);
}

/** Called by renderSite; reads git again when another app is shown. */
window.renderGit = (s) => {
  if (gitShown !== s.name || !gitCache.has(s.name)) loadGit(s.name);
  gitShown = s.name;
  drawGit(s.name);
};

// The reload button and coming back to the window (after a commit elsewhere) read git again.
$("#reload").addEventListener("click", () => gitCache.clear());
window.addEventListener("focus", () => {
  if (mode === "apps" && selected) loadGit(selected);
});

// The first state may arrive before this file loads.
if (snap && mode === "apps") renderSite();
