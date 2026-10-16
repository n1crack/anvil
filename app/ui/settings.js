const { invoke } = window.__TAURI__.core;

const $ = (sel, root = document) => root.querySelector(sel);
const tilde = (p) => p.replace(/^\/Users\/[^/]+/, "~");
let s = null;

async function call(cmd, args) {
  try {
    s = (await invoke(cmd, args)) ?? s;
    showError(null);
    render();
  } catch (e) {
    showError(String(e));
  }
}

function showError(msg) {
  $("#error").textContent = msg ?? "";
  $("#error").hidden = !msg;
}

window.refresh = () => call("settings");

function options(select, values, current, none) {
  select.replaceChildren(new Option(none, ""));
  for (const v of values) select.add(new Option(v, v));
  // Keep a configured app selectable even if it isn't installed where we look.
  if (current && !values.includes(current)) select.add(new Option(current, current));
  select.value = current ?? "";
}

function render() {
  // Site folders
  const paths = $("#paths");
  paths.replaceChildren();
  if (!s.paths.length) {
    paths.innerHTML = `<div class="row"><span class="muted">No folders yet.</span></div>`;
  }
  for (const p of s.paths) {
    const row = document.createElement("div");
    row.className = "row";
    row.innerHTML = `<span class="path"></span><button>Remove</button>`;
    $(".path", row).textContent = tilde(p);
    $("button", row).onclick = () => call("remove_path", { path: p });
    paths.append(row);
  }

  $("#login").checked = s.login;
  $("#dock").checked = s.dock_icon;
  $("#theme").value = s.theme ?? "";
  options($("#editor"), s.editors, s.editor, "None");
  options($("#terminal"), s.terminals.filter((t) => t !== "Terminal"), s.terminal, "Terminal");

  $("#tld").textContent = `.${s.tld}`;
  $("#http").textContent = s.http_port;
  $("#https").textContent = s.https_port;
  $("#dns").textContent = s.dns_port;
  $("#config-dir").textContent = tilde(s.config_dir);

  // PHP versions
  const versions = $("#versions");
  versions.replaceChildren();
  for (const v of s.php) {
    const row = document.createElement("div");
    row.className = "row";
    const label = document.createElement("span");
    label.textContent = `PHP ${v.version}`;
    const actions = document.createElement("span");
    actions.className = "actions-inline";
    if (v.default) {
      const tag = document.createElement("span");
      tag.className = "tag";
      tag.textContent = "Default";
      actions.append(tag);
    } else if (v.installed) {
      actions.append(
        button("Make default", () => call("set_default_php", { version: v.version })),
        button("Uninstall", () => call("php_uninstall", { version: v.version })),
      );
    } else {
      actions.append(button("Install", async (b) => {
        b.disabled = true;
        b.textContent = "Installing…";
        await call("php_install", { version: v.version });
      }));
    }
    row.append(label, actions);
    versions.append(row);
  }

  for (const input of document.querySelectorAll("[data-ini]")) {
    if (document.activeElement !== input) input.value = s.ini[input.dataset.ini] ?? "";
  }
}

function button(text, onclick) {
  const b = document.createElement("button");
  b.textContent = text;
  b.onclick = () => onclick(b);
  return b;
}

$("#add-path").onclick = async () => {
  const path = await window.__TAURI__.dialog.open({ directory: true, title: "Choose a folder of projects" });
  if (path) call("add_path", { path });
};
$("#login").onchange = (e) => call("set_login", { on: e.target.checked });
$("#dock").onchange = (e) => call("set_dock_icon", { on: e.target.checked });
$("#theme").onchange = (e) => call("set_theme", { theme: e.target.value || null });
$("#editor").onchange = (e) => call("set_editor", { editor: e.target.value || null });
$("#terminal").onchange = (e) => call("set_terminal", { terminal: e.target.value || null });
$("#open-config").onclick = () => invoke("open_config_dir").catch((e) => showError(String(e)));

for (const input of document.querySelectorAll("[data-ini]")) {
  input.onchange = async () => {
    const value = input.value.trim() || null;
    await call("set_ini", { site: null, key: input.dataset.ini, value });
    // An upload can't be bigger than the POST body that carries it.
    if (input.dataset.ini === "upload_max_filesize") {
      await call("set_ini", { site: null, key: "post_max_size", value });
    }
  };
}

for (const b of document.querySelectorAll("[data-page]")) {
  b.onclick = () => {
    for (const other of document.querySelectorAll("[data-page]")) other.setAttribute("aria-current", other === b);
    for (const page of document.querySelectorAll(".page")) page.hidden = page.id !== b.dataset.page;
  };
}

document.addEventListener("contextmenu", (e) => {
  if (!e.target.closest("input, textarea")) e.preventDefault();
});

window.refresh();
