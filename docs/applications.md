# Applications

Anvil looks at the files in each folder to decide what kind of application it
is. You can override the type with `anvil app type <site> <kind>` or in the app.

| Type | Detected by | Served by |
|---|---|---|
| Laravel | `artisan` | PHP-FPM |
| Statamic | `artisan` and `statamic/cms` in `composer.json` | PHP-FPM |
| Symfony | `bin/console` and `symfony/framework-bundle` in `composer.json` | PHP-FPM |
| PHP | `composer.json`, `index.php` or `public/index.php` | PHP-FPM |
| Rust | `Cargo.toml` | `cargo run` |
| Go | `go.mod` | `go run .` |
| .NET | a `.csproj`, `.fsproj` or `.sln` file | `dotnet run` |
| Next.js | `next` in `package.json` | `npm run dev` |
| Nuxt | `nuxt` in `package.json` | `npm run dev` |
| Vite | `vite` in `package.json` | `npm run dev` |
| Node.js | `package.json` | `npm run dev`, or `npm run start` without a `dev` script |
| Django | `manage.py` that imports Django | `python manage.py runserver` |
| FastAPI | `fastapi` in `pyproject.toml` or `requirements.txt` (with `package.json`, also `main.py`, `app.py` or `wsgi.py`) | `uvicorn` |
| Flask | `flask` in `pyproject.toml` or `requirements.txt` (same rule) | `flask run` |
| Static | anything else | the web server |

- **PHP applications** are served through PHP-FPM. The web root is `public/`
  when it exists, otherwise the folder itself.
- **Other applications** run their own dev server on a private port. Anvil
  passes the port in `$PORT` (and `ASPNETCORE_URLS` for .NET) and proxies
  `<name>.test` to it. Node.js projects install their packages first if
  `node_modules` is missing. You can change the start command and the port per
  application.
- **A stopped application** shows a "not running" page at its domain instead of
  an error.
- Node.js picks the version from `.nvmrc` or `.node-version` when it is
  installed; otherwise the newest installed version is used. Pin another one
  with `anvil app runtime`.
- **Python applications** run through [uv](https://docs.astral.sh/uv/), which
  Anvil installs (checked against a pinned checksum) together with the Python
  versions in `~/.anvil`. Environments live in `~/.anvil/cache`, so a `.venv`
  you already have is left alone. A `pyproject.toml` project (one with a
  `[project]` table) gets a `uv.lock` written next to it, as `uv run` does; a
  project with only `requirements.txt` gets nothing added. The Python version
  follows the project's `.python-version` and `requires-python` (uv installs a
  missing one into `~/.anvil`) unless you pin one in the app.
- **Django** checks the host name and, behind HTTPS, the scheme. The app's
  Overview shows the two lines to add to `settings.py` once:

  ```python
  if DEBUG:  # Anvil
      ALLOWED_HOSTS += [".test"]
      SECURE_PROXY_SSL_HEADER = ("HTTP_X_FORWARDED_PROTO", "https")
  ```

  Without them Django answers 400 Bad Request for `.test` addresses and forms
  fail their CSRF check. (With `.localhost`, Django's debug mode already
  accepts the host, but forms still need the proxy line.) They only apply in
  debug, so a deployed site keeps its own settings.

## PHP

PHP 8.0 to 8.5 are portable builds installed into `~/.anvil/php/<version>`.
Set a default version for everything (`anvil use 8.4`) and pin single
applications to another one (`anvil use 8.3 --site myapp`). Memory limit,
upload limit, execution time and any other php.ini value can be set for all
applications (`anvil php ini`) or for one (`anvil ini`); an application with
its own settings gets its own PHP-FPM pool.

## Languages

Node.js, Go, Rust (through rustup), .NET and Python (through uv) are installed into
`~/.anvil/runtimes` and only put on `PATH` for the applications Anvil runs.
Their package and build caches go to `~/.anvil/cache`, so `~/.npm`, `~/go`,
`~/.cargo` and `~/.nuget` stay untouched. So do the caches of tools those
runtimes run: node-gyp, Corepack, and anything that follows `XDG_CACHE_HOME`
or `XDG_STATE_HOME` (giget, yarn, pnpm's state), unless you set those
variables yourself. Next.js and Nuxt telemetry is off.

Two things stay where the tools themselves keep them:

- **Go telemetry.** Go writes small local usage counters to
  `~/Library/Application Support/go/telemetry`, the folder your own Go uses
  too. Nothing is uploaded unless you ran `go telemetry on`. Go only lets the
  home folder move it, and changing that would hide your `~/.gitconfig`,
  `~/.netrc` and Go settings from builds, so Anvil leaves it. `anvil uninstall`
  doesn't remove it, since it isn't only Anvil's.
- **Shared package stores** a project already uses, like pnpm's store in
  `~/Library/pnpm`: moving it would make pnpm refuse the project's existing
  `node_modules`.

```sh
anvil runtime list
anvil runtime install node 22
```

## New applications

Create a project from a template with `anvil new <kind> <name>` or the New
Application sheet in the app: Laravel, Statamic, Symfony, PHP, static, Next.js,
Nuxt, Vite, Node.js, Rust, Go and .NET. Anvil uses its own PHP, Composer and
language toolchains, so the matching PHP version or language must be installed
first. Output goes to `~/.anvil/log/create-<name>.log`, and a failed create
leaves nothing behind. A new application outside a parked folder is linked.

## Storage

Anvil measures each application's disk use and sorts it into three groups:

- **Safe:** logs and caches. Delete any time.
- **Rebuildable:** dependencies and builds such as `node_modules`, `vendor`,
  `target` or `.next`. A command brings them back.
- **Keep:** your code and data, such as `storage/app` or a SQLite database.
  Anvil never deletes these.

In the app you can clean an application item by item, or clean the safe items
of every application at once. Rebuildable items can only be deleted while the
application is stopped. `anvil usage` shows the same numbers in the terminal,
along with the services an application's `.env` points at, and `anvil clean`
deletes them.
