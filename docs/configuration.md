# Configuration

Settings live in `~/.anvil/config.toml`. The CLI commands and the macOS app
both write this file, so you rarely need to edit it by hand. Every field is
optional; a missing field takes its default. After editing it by hand, restart
`anvil start` (or use Reload in the app).

## Top-level fields

| Field | Default | Meaning |
|---|---|---|
| `tld` | `"test"` | Domain suffix: sites answer at `<name>.<tld>` |
| `http_port` | `8080` | HTTP port of the web server |
| `https_port` | `8443` | HTTPS port of the web server |
| `dns_port` | `5354` | Port of Anvil's DNS server on 127.0.0.1 |
| `php` | none | Default PHP version, e.g. `"8.4"`; the first `anvil php install` sets it (`anvil use`) |
| `paths` | `[]` | Parked folders: every sub-folder is a site (`anvil park`) |
| `links` | `{}` | Linked sites: name to folder (`anvil link`) |
| `hidden` | `[]` | Parked sites removed from the list; their folders are left alone |
| `groups` | `{}` | Your groups in the app's list: group name to site names |
| `ini` | `{}` | php.ini values for every site (`anvil php ini`) |
| `editor` | none | macOS app used by "Open in editor", e.g. `"PhpStorm"` |
| `terminal` | none | macOS app used by "Open Terminal", e.g. `"iTerm"` |
| `git_client` | none | Git app for the app's Git button, e.g. `"Fork"`; unset uses the first one installed |
| `theme` | none | `"light"` or `"dark"`; unset follows the system |
| `dock_icon` | `false` | Keep the Dock icon while no window is open |

If you change `tld` or `dns_port`, update `/etc/resolver/<tld>` to match (see
[getting-started.md](getting-started.md#resolver-setup)).

## Per-site settings: `[sites."<name>"]`

| Field | Meaning |
|---|---|
| `php` | PHP version for this site (`anvil use <version> --site <name>`) |
| `ini` | php.ini values for this site only; the site gets its own PHP-FPM pool (`anvil ini`) |
| `kind` | Override the detected type, e.g. `"vite"` (`anvil app type`) |
| `command` | Start command for apps with their own server (`anvil app command`) |
| `port` | Fixed private port for apps with their own server (`anvil app port`) |
| `autostart` | Start the app with Anvil (`anvil app autostart`) |
| `runtime` | Language version for apps with their own server, e.g. Node `"24"` (`anvil app runtime`) |

## Services: `[services."<name>"]`

| Field | Meaning |
|---|---|
| `kind` | `mysql`, `postgres`, `redis` or `mailpit` |
| `version` | Program version, fixed when the service is added; unset means the kind's default |
| `enabled` | Start with Anvil |
| `port` | Port on 127.0.0.1 |
| `web_port` | Mailpit's web inbox port (default 8025) |
| `tags` | Free-form labels, e.g. `["queue"]` |
| `apps` | Applications that use the service, by site name |

Use `anvil service add` rather than adding a service by hand: it picks a free
port and installs the program. See [services.md](services.md).

## Example

```toml
tld = "test"
http_port = 8080
https_port = 8443
dns_port = 5354
php = "8.4"
editor = "PhpStorm"
terminal = "iTerm"
paths = ["/Users/me/Sites"]

[links]
shop = "/Users/me/code/shop"

[ini]               # php.ini defaults for every site
memory_limit = "512M"

[sites."myapp"]
php = "8.3"

[sites."myapp".ini]
memory_limit = "1G" # this site gets its own PHP-FPM pool

[sites."dashboard"]
command = "npm run dev"
port = 41500
autostart = true

[services.mysql]
kind = "mysql"
enabled = true
port = 3306

[services.queue]
kind = "redis"
enabled = true
port = 6380
tags = ["queue"]
apps = ["myapp"]
```

## The `~/.anvil` folder

Everything Anvil installs and generates lives here. Set `ANVIL_HOME` to use
another folder.

| Path | Contents |
|---|---|
| `config.toml` | Your settings |
| `php/<version>/` | PHP binaries and `php.ini` |
| `runtimes/<id>/` | Node.js, Go, Rust, .NET, Python and uv |
| `services/<kind>/<version>/` | MySQL, PostgreSQL, Redis and Mailpit programs, shared by every service of that version |
| `data/<name>/` | Each service's data |
| `data/caddy/` | Certificate authority and site certificates |
| `bin/` | Web server and Composer |
| `cache/` | Package and build caches for the languages and Composer |
| `run/` | Generated configuration, sockets and pid files |
| `log/` | Logs of the web server, PHP-FPM, services, apps and `anvil new` |
