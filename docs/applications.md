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

## PHP

PHP 8.0 to 8.5 are portable builds installed into `~/.anvil/php/<version>`.
Set a default version for everything (`anvil use 8.4`) and pin single
applications to another one (`anvil use 8.3 --site myapp`). Memory limit,
upload limit, execution time and any other php.ini value can be set for all
applications (`anvil php ini`) or for one (`anvil ini`); an application with
its own settings gets its own PHP-FPM pool.

## Languages

Node.js, Go, Rust (through rustup) and .NET are installed into
`~/.anvil/runtimes` and only put on `PATH` for the applications Anvil runs.
Their package and build caches go to `~/.anvil/cache`, so `~/.npm`, `~/go`,
`~/.cargo` and `~/.nuget` stay untouched.

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
