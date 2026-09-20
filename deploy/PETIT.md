# Shared Petit operations

Petit owns all four schedules. Systemd still owns process identity, credentials,
resource limits and isolation. Install `petit/*.yaml` in `/etc/petit-bots/jobs` and
`60-chicago-data-bot.rules` in `/etc/polkit-1/rules.d`. The rule grants only `start`
for this bot's four named services to the existing scheduler user, ubuntu.

| Job | Schedule |
| --- | --- |
| Worker | Weekdays 08:03, 14:03, 18:03 America/Chicago |
| Deployment | UTC minutes 11, 26, 41, 56 |
| Health | Hourly at minute 48 UTC |
| Backup | Daily at 04:47 UTC |

Disable `chicago-data-bot.timer` and `chicago-data-bot-deploy.timer`. Keep their
files for rollback only. Never run both scheduling paths. Each Petit command uses
`systemctl start --wait`, so the shared history records completion and failures.
Do not run a second scheduler against its history database.

Drain shared jobs before restarting `petit-bots.service` to load configuration.
Its existing startup hooks perform Bikeshare recovery/catch-up; avoid unnecessary
restarts. Do not replace the shared service or change other bots' job definitions.

The bot's `check` command requires an existing, valid baseline and opens SQLite
read-only. `check --health` also checks scheduled scan freshness with a 30-minute
grace period and queue age relative to scheduled runs. Weekend gaps are expected.
`backup DIRECTORY` uses SQLite's online backup API, verifies the copied database,
and atomically saves it, retaining seven copies. Backups contain publication state;
keep them private. The daily service writes `/var/lib/chicago-data-bot/backups`.
Off-host backup storage is not configured.

CI pins Rust 1.94.0, tests Linux and macOS, and builds native Linux release assets
on GitHub. The existing Rust updater verifies checksums and atomically switches the
runtime symlink. Packaging uses standard shell tools; no Python or credentials are
needed in Actions. Production credentials remain root-only in
`/etc/chicago-data-bot/.env`. Updater and privileged unit changes need operator
installation. This migration changes no database schema; future incompatible schema
changes require an explicit migration and compatibility validation before release.

Inspect jobs using `/opt/petit-bots/pt list /etc/petit-bots/jobs`, systemd journals,
and the existing Petit history database. Manual `systemctl start --wait UNIT`
uses the same service action as Petit, but does not add a Petit history entry.

For rollback: drain and remove only this bot's Petit jobs, restart the drained
scheduler, and enable its two previous timers. Pause deployment before selecting
a previous compatible executable. Keep the current database and receipts; never
restore an older database just to revert code.
