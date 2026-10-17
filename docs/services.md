# Services

Databases, cache and mail run as Anvil services. Nothing runs until you add it.
Add a service by picking its kind, a name and a port, and add as many of each
as you need: two MySQL servers for two projects, a separate Redis for queues,
a second Mailpit inbox.

```sh
anvil service add mysql                      # named "mysql", usual port
anvil service add redis cache --port 6380    # a second Redis named "cache"
anvil service add mysql legacy --version 8.0.46
anvil service list
```

In the macOS app, Services sits next to Applications in the sidebar; the +
button (⌘N) adds one. New services get a readable name such as
`redis-swift-ember`; you can type your own.

## Available services

| Service | Versions | Usual port |
|---|---|---|
| MySQL | 9.7, 8.4 (default), 8.0 | 3306 |
| PostgreSQL | 18 (default), 17, 16, 15, 14 | 5432 |
| Redis (built from source) | 8.10 (default), 7.4, 7.2 | 6379 |
| Mailpit | 1.31 | 1025 (SMTP), web inbox on 8025 |

`anvil service versions` lists the exact versions and which are installed.
Redis is compiled on your Mac and needs the Xcode command line tools.

## Versions and data

A service keeps the version it was added with, like a container image: data
folders don't survive a major version change, so run a new version as a new
service next to the old one.

Each version's program is downloaded the first time you add a service with it
and shared after that, in `~/.anvil/services/<kind>/<version>`. Every service
keeps its own data in `~/.anvil/data/<name>` and its own log in `~/.anvil/log`.
`anvil service uninstall <kind> <version>` removes a program version once no
service uses it.

Removing a service keeps its data unless you pass `--delete-data` (or choose
to delete it in the app), so adding the same name again finds your databases.

## Ports

When the usual port is taken, by another service or another program, the next
free one is suggested. Change it with `anvil service port <name> <port>`;
leave the port out to go back to the usual one. Mailpit's web inbox port is
set with `anvil service port <name> --web <port>`.

## Accounts

Services listen on 127.0.0.1 only.

- MySQL starts with a `root` user without a password.
- PostgreSQL starts with a `postgres` user and trust authentication.

## Starting and stopping

Enabled services start with Anvil (`anvil start` or the app);
`anvil service enable <name>` and `disable <name>` choose which. In the app
each service also has Start, Stop and Restart.

## Connecting applications

Tag services (`anvil service tag <name> queue`) and attach them to the
applications that use them (`anvil service link <name> myapp`). Each service
shows the `.env` lines to connect in the app.

For each application, Anvil also reads its `.env` and tells you where every
database, cache and mail server it points at comes from: an Anvil service, a
Docker container or Compose service, another program on your Mac, or a remote
host. `anvil usage <site>` shows the same in the terminal.

## What's inside

For a running service, `anvil service databases <name>` lists MySQL and
PostgreSQL databases with their sizes, Redis keys and memory, or Mailpit
messages. Create and drop databases with `anvil service create-db` and
`drop-db`. The app can also flush Redis, delete all Mailpit messages and open
a service in a database app you have installed (TablePlus, Sequel Ace,
Postico, DBeaver, DataGrip, Beekeeper Studio, RedisInsight).
