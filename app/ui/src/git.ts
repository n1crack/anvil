// @ts-nocheck
import { $, drawMeta, el, invoke, mode, open, renderSite, row, selected, showError, showTab, snap, tab } from "./app";
import { late } from "./late";

// Git: a button for the git app and a Git group in the Overview.

export const gitCache = new Map(); // site name -> info, null when not a repository
export let gitShown = null;

export async function loadGit(name) {
  try {
    gitCache.set(name, await invoke("git_info", { site: name }));
  } catch (e) {
    gitCache.set(name, { error: String(e) });
  }
  if (selected === name) {
    drawGit(name);
    drawMeta();
  }
}

export function commitAgo(secs) {
  const rtf = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
  const d = secs - Date.now() / 1000;
  for (const [unit, n] of [["year", 31536000], ["month", 2592000], ["week", 604800], ["day", 86400], ["hour", 3600], ["minute", 60]]) {
    if (Math.abs(d) >= n) return rtf.format(Math.round(d / n), unit);
  }
  return "just now";
}

export function drawGit(name) {
  const root = $("#site");
  const panel = $('[data-panel="git"]', root);
  if (!panel) return;
  const g = gitCache.get(name);
  const repo = g && !g.error;
  // The tab only exists for repositories.
  const tabBtn = $('.tabs [data-tab="git"]', root);
  // Hidden until git answers; only a folder known not to be a repository leaves the tab.
  tabBtn.hidden = !repo;
  if (g && !repo && tab === "git") showTab(root, "overview");
  panel.replaceChildren();
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
    const when = el("span", "muted", ` ${c.hash}, ${c.author}, ${commitAgo(c.time)}`);
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
  panel.append(list);

  // The git app lives here, not among the buttons at the top.
  if (snap.git_client) {
    const actions = el("div", "below");
    const btn = el("button", "git-btn", `Open in ${snap.git_client}`);
    btn.title = "⌘G";
    btn.onclick = () => open("git");
    actions.append(btn);
    panel.append(actions);
  }
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

late.gitCache = gitCache;
