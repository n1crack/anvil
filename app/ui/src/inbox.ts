/* One inbox of the mail catcher (src/mail.rs): its messages newest first, each opening to
 * its HTML (sandboxed), text, headers or source, with its files to save. The Mail tab
 * shows an app's (mail.ts); Settings › Features the one for mail from no app. Only
 * `invoke` comes in, so both windows use it. */

import { confirmInRow } from "./confirm";
import { knob } from "./knob";

type Invoke = <T = any>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

function el(tag: string, cls?: string | null, text?: string | number | null): any {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = String(text);
  return e;
}

/** "812 B", "5.2 KB", "1.4 MB". */
function size(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1048576).toFixed(1)} MB`;
}

const CLOCK = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");
const CLIP =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10.5 4.5 5.8 9.2a1.25 1.25 0 0 0 1.8 1.8l5-5a2.5 2.5 0 0 0-3.6-3.6l-5 5a3.75 3.75 0 0 0 5.3 5.3l4.2-4.2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>';

/** The HTML view's document: no scripts (the sandbox), and nothing loaded from the
 * network (its CSP) until the user asks for remote images. */
export function srcdoc(html: string, remote: boolean) {
  const csp = `default-src 'none'; img-src data:${remote ? " https: http:" : ""}; style-src 'unsafe-inline'`;
  // A refresh would take the frame elsewhere.
  const body = html.replace(/<meta[^>]+http-equiv\s*=\s*["']?refresh[^>]*>/gi, "");
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}"><style>body{margin:12px;font:14px/1.45 -apple-system,system-ui,sans-serif;background:#fff;color:#111}</style></head><body>${body}</body></html>`;
}

/** Whether the HTML loads anything from the network: images, backgrounds, CSS urls. */
export const remoteRefs = (html: string) => /(?:src|background)\s*=\s*["']?\s*https?:|url\(\s*["']?\s*https?:/i.test(html);

export interface Inbox {
  inbox: string;
  node: HTMLElement;
  load(): Promise<void>;
  tick(): Promise<void>;
}

export function inboxView(inbox: string, invoke: Invoke, emptyText: string): Inbox {
  const node = el("div", "logs mail");
  node.innerHTML = `
    <div class="logs-bar mail-bar"><span class="mail-count muted"></span><span class="actions-inline mail-clear-box"><button class="mail-clear" hidden>Clear</button></span></div>
    <p class="error mail-err" hidden></p>
    <ol class="logs-list mail-list"></ol>`;
  const list = node.querySelector(".mail-list");
  const err = node.querySelector(".mail-err");
  const clear = node.querySelector(".mail-clear");
  const fail = (e: unknown) => {
    err.textContent = String(e);
    err.hidden = false;
  };
  let ids: string[] = [];

  const count = () => {
    const n = list.querySelectorAll(".mail-item").length;
    node.querySelector(".mail-count").textContent = n ? `${n} ${n === 1 ? "message" : "messages"}` : "";
    clear.hidden = !n;
    if (!n && !list.querySelector(".empty")) list.replaceChildren(el("li", "empty", emptyText));
    if (n) list.querySelector(".empty")?.remove();
  };

  /** One row: time, from, to, subject, files, size. */
  const row = (m) => {
    const li = el("li", "log mail-item");
    li.dataset.id = m.id;
    const head = el("button", "log-head");
    head.setAttribute("aria-expanded", "false");
    const d = new Date(m.ts);
    const time = el("span", "log-time", CLOCK.format(d));
    time.title = d.toLocaleString();
    const subject = el("span", m.subject ? "log-msg" : "log-msg muted", m.subject || "(no subject)");
    subject.title = m.subject;
    const files = el("span", "mail-files");
    if (m.attachments) {
      files.innerHTML = CLIP;
      files.append(String(m.attachments));
      files.title = `${m.attachments} ${m.attachments === 1 ? "file" : "files"}`;
    }
    const from = el("span", "mail-from", m.from);
    from.title = m.from;
    const to = el("span", "mail-to", m.to.join(", "));
    to.title = to.textContent;
    head.append(time, from, to, subject, files, el("span", "req-size", size(m.size)));
    head.onclick = () => toggle(li);
    li.append(head);
    return li;
  };

  const toggle = async (li) => {
    const head = li.querySelector(".log-head");
    const open = head.getAttribute("aria-expanded") !== "true";
    head.setAttribute("aria-expanded", String(open));
    if (!open) return li.querySelector(".log-detail")?.remove();
    let m;
    try {
      m = await invoke("mail_read", { inbox, id: li.dataset.id });
    } catch (e) {
      return fail(e);
    }
    if (head.getAttribute("aria-expanded") === "true" && !li.querySelector(".log-detail")) li.append(detail(li, m));
  };

  const detail = (li, m) => {
    const box = el("div", "log-detail mail-detail");
    const views: [string, string][] = [];
    if (m.html) views.push(["html", "HTML"]);
    if (m.text) views.push(["text", "Text"]);
    views.push(["headers", "Headers"], ["source", "Source"]);
    const body = el("div", "mail-body");
    let remote = false;
    const tools = el("div", "mail-actions");
    const show = async (id: string) => {
      tools.querySelector(".mail-remote")?.remove();
      if (id === "html") {
        const f = el("iframe", "mail-html");
        // No allow-scripts: nothing in the mail runs. Same origin only so its links can
        // open in the browser and the frame can take the mail's height.
        f.setAttribute("sandbox", "allow-same-origin");
        f.title = "The mail as HTML";
        f.srcdoc = srcdoc(m.html, remote);
        f.onload = () => {
          const doc = f.contentDocument;
          if (!doc) return;
          f.style.height = `${doc.documentElement.scrollHeight}px`;
          doc.addEventListener("click", (e) => {
            const a = (e.target as Element)?.closest?.("a[href]") as HTMLAnchorElement | null;
            if (!a) return;
            e.preventDefault();
            if (/^https?:/.test(a.href)) invoke("open_url", { url: a.href }).catch(fail);
          });
        };
        body.replaceChildren(f);
        if (!remote && remoteRefs(m.html)) {
          const b = el("button", "mail-remote", "Load remote images");
          b.onclick = () => {
            remote = true;
            b.remove();
            f.srcdoc = srcdoc(m.html, true);
          };
          tools.prepend(b);
        }
      } else if (id === "text") body.replaceChildren(el("pre", "mail-text", m.text));
      else if (id === "headers") {
        const dl = el("dl", "req-headers");
        for (const [k, v] of m.headers) {
          const line = el("div");
          line.append(el("dt", null, k), el("dd", null, v));
          dl.append(line);
        }
        body.replaceChildren(dl);
      } else {
        const pre = el("pre", "mail-text muted", "Reading…");
        body.replaceChildren(pre);
        try {
          pre.textContent = await invoke("mail_source", { inbox, id: m.id });
          pre.classList.remove("muted");
        } catch (e) {
          pre.textContent = String(e);
        }
      }
    };
    const pick = el("div", "mail-views");
    const segs = el("div", "seg");
    segs.setAttribute("role", "group");
    segs.setAttribute("aria-label", "Show");
    for (const [id, text] of views) {
      const b = el("button", null, text);
      b.dataset.id = id;
      b.setAttribute("aria-pressed", String(id === views[0][0]));
      b.onclick = () => {
        for (const o of segs.querySelectorAll("button")) o.setAttribute("aria-pressed", String(o === b));
        show(id);
      };
      segs.append(b);
    }
    knob(segs);
    const del = el("button", "mail-delete", "Delete");
    // Asks where the buttons were, as Clear does in the bar.
    del.onclick = () => {
      const had = [...tools.children];
      const back = () => {
        tools.classList.remove("asking");
        tools.replaceChildren(...had);
      };
      confirmInRow(
        tools,
        "Delete this message?",
        "Delete",
        async () => {
          try {
            await invoke("mail_delete", { inbox, id: m.id });
            li.remove();
            ids = ids.filter((x) => x !== m.id);
            count();
          } catch (e) {
            back();
            fail(e);
          }
        },
        back,
      );
    };
    tools.append(del);
    pick.append(segs, tools);
    box.append(pick);
    const files = m.attachments.filter((a) => !a.inline);
    if (files.length) {
      const fl = el("div", "mail-files-list");
      for (const a of files) {
        const f = el("div", "mail-file");
        const save = el("button", "inline-action", "Save");
        save.onclick = async () => {
          save.disabled = true;
          try {
            const path = await invoke<string>("mail_save", { inbox, id: m.id, index: a.index });
            const said = el("span", "muted", `Saved to ${tilde(path)}`);
            said.setAttribute("role", "status");
            save.replaceWith(said);
          } catch (e) {
            const why = el("span", "error", String(e));
            why.setAttribute("role", "status");
            save.replaceWith(why);
          }
        };
        f.append(el("span", null, `${a.name} ${size(a.size)}`), " ", save);
        fl.append(f);
      }
      box.append(fl);
    }
    box.append(body);
    show(views[0][0]);
    return box;
  };

  const load = async () => {
    let mails;
    try {
      mails = await invoke("mail_list", { inbox });
    } catch (e) {
      return fail(e);
    }
    err.hidden = true;
    ids = mails.map((m) => m.id);
    list.replaceChildren(...mails.map(row));
    count();
  };

  /** What came in or went (pruned, deleted elsewhere) since: open rows stay open. */
  const tick = async () => {
    let mails;
    try {
      mails = await invoke("mail_list", { inbox });
    } catch {
      return;
    }
    const now = mails.map((m) => m.id);
    if (now.join() === ids.join()) return;
    const known = new Set(ids);
    const keep = new Set(now);
    for (const li of list.querySelectorAll(".mail-item")) if (!keep.has(li.dataset.id)) li.remove();
    list.prepend(...mails.filter((m) => !known.has(m.id)).map(row));
    ids = now;
    count();
  };

  // Asks in the bar, where Clear was.
  clear.onclick = () => {
    const box = clear.parentElement;
    const n = ids.length;
    const back = () => {
      box.classList.remove("asking");
      box.replaceChildren(clear);
    };
    confirmInRow(
      box,
      `Delete ${n === 1 ? "1 message" : `all ${n} messages`}? This can’t be undone.`,
      "Delete",
      async () => {
        try {
          await invoke("mail_clear", { inbox });
        } catch (e) {
          fail(e);
        }
        back();
        load();
      },
      back,
    );
  };

  return { inbox, node, load, tick };
}
