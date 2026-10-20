// The libraries compiled into Anvil, for Settings › About › Credits: the Rust crates
// the CLI and the app link (normal dependencies, macOS and Windows targets; not build or dev
// ones) and the npm packages the UI ships (`--prod`). Writes public/credits.json
// (name, version, license, link) and public/THIRD_PARTY_NOTICES.txt (each crate's
// license files as published), both bundled with the UI. Run by `pnpm build`.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ui = join(dirname(fileURLToPath(import.meta.url)), "..");
// Windows: pnpm is a .cmd, which only a shell runs.
const run = (cmd, args, cwd) =>
  execFileSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 1 << 28, shell: process.platform === "win32" });

const meta = JSON.parse(
  run(
    "cargo",
    ["metadata", "--format-version=1", "--locked", "--filter-platform=aarch64-apple-darwin", "--filter-platform=x86_64-apple-darwin", "--filter-platform=x86_64-pc-windows-msvc"],
    join(ui, "..", ".."),
  ),
);
const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const members = new Set(meta.workspace_members);
const seen = new Set();
const todo = [...members];
while (todo.length) {
  const id = todo.pop();
  for (const d of nodes.get(id)?.deps ?? []) {
    if (!seen.has(d.pkg) && d.dep_kinds.some((k) => k.kind === null)) {
      seen.add(d.pkg);
      todo.push(d.pkg);
    }
  }
}

const crates = [...seen]
  .filter((id) => !members.has(id))
  .map((id) => pkgs.get(id))
  .map((p) => ({
    name: p.name,
    version: p.version,
    license: p.license ?? (p.license_file ? "see notices" : "unknown"),
    url: p.repository ?? p.homepage ?? `https://crates.io/crates/${p.name}`,
    dir: dirname(p.manifest_path),
  }));

// `pnpm licenses list --json`: `{ "<license>": [{ name, versions, homepage }] }`.
const npm = Object.entries(JSON.parse(run("pnpm", ["licenses", "list", "--prod", "--json"], ui) || "{}")).flatMap(([license, list]) =>
  list.flatMap((p) =>
    p.versions.map((version, i) => ({
      name: p.name,
      version,
      license,
      url: p.homepage ?? `https://www.npmjs.com/package/${p.name}`,
      dir: p.paths?.[i],
    })),
  ),
);

const byName = (a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version);
const list = { rust: crates.sort(byName), npm: npm.sort(byName) };

const texts = (dir) => {
  if (!dir) return [];
  try {
    return readdirSync(dir)
      .filter((f) => /^(licen[cs]e|copying|notice|unlicense)/i.test(f))
      .sort()
      .map((f) => readFileSync(join(dir, f), "utf8").trim());
  } catch {
    return [];
  }
};
// Packages with the very same license files share one copy of them.
const groups = new Map();
for (const p of [...list.rust, ...list.npm]) {
  const body = texts(p.dir).join("\n\n---\n\n") || "No license file in the published package; the license is the one named above.";
  groups.set(body, [...(groups.get(body) ?? []), p]);
}
const notices = [...groups]
  .map(([body, ps]) => `${ps.map((p) => `${p.name} ${p.version} (${p.license}) ${p.url}`).join("\n")}\n\n${body}`)
  .join(`\n\n${"=".repeat(78)}\n\n`);

const strip = (p) => ({ name: p.name, version: p.version, license: p.license, url: p.url });
writeFileSync(join(ui, "public", "credits.json"), `${JSON.stringify({ rust: list.rust.map(strip), npm: list.npm.map(strip) })}\n`);
writeFileSync(
  join(ui, "public", "THIRD_PARTY_NOTICES.txt"),
  `Libraries compiled into Anvil, with their licenses as published.\n\n${notices}\n`,
);
console.log(`credits: ${list.rust.length} crates, ${list.npm.length} npm packages`);
