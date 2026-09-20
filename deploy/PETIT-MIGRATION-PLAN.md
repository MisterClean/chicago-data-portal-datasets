# Petit migration plan — approved September 20, 2026

## Observed on September 20, 2026

Read-only SSH inspection confirmed that the bot is already deployed. Production runs
`build-350e0236011ce8403fd6ea6cd6b89e511f65d389`, including the updated announcement renderer.
The Rust updater is successfully polling GitHub releases. Both existing systemd timers
are enabled. Worker checks run weekdays at 08:00, 14:00 and 18:00 America/Chicago.

The SQLite database at `/var/lib/chicago-data-bot/catalog.sqlite` contains 915 known IDs,
zero pending announcements, and zero production post receipts. Its baseline dates to
September 17; the last successful scan was September 18 at 23:01:13 UTC (18:01 Chicago).
The weekend gap is expected. Local test posts are outside this production outbox.

Credentials already exist at `/etc/chicago-data-bot/.env`, owned by root with mode 600.
Their contents were not printed. Only `.env.example` is tracked; local `.env` is ignored.

Petit runs as ubuntu with two concurrent jobs and uses
`/var/lib/petit-chicago-bikeshare-bot/history.sqlite3` for history. ADU jobs invoke
`systemctl start --wait` for worker, updater, health, and backup services. A Polkit rule
permits ubuntu to start only those named units. Recent ADU runs completed successfully.

The host has 412 MiB RAM, approximately 119 MiB available, nearly full 2 GiB swap,
and 2.6 GiB free disk. Build on GitHub; retain short-lived native executables and limits.

## Repository work after approval

1. Add four Petit job definitions and a separate narrowly scoped Polkit rule for this
   bot. Preserve dedicated systemd identities, environment loading, and sandboxing.
   Petit must not receive the Bluesky credentials or direct database write permission.
2. Keep worker checks at 08:00, 14:00 and 18:00 Chicago weekdays, using a small fixed
   minute offset to reduce collisions. Keep release checks every 15 minutes, hourly
   health checks, and one daily backup; choose offsets around the existing jobs.
   Use `max_concurrency: 1`, blocking service starts, and Petit timeouts longer than
   the corresponding systemd timeout. Validate weekday/DST semantics with the installed
   Petit version before activation.
3. Add Rust read-only health and SQLite-consistent backup commands, with dedicated
   services. Health checks must check integrity, baseline, overdue scheduled scans,
   and pending queue age without falsely flagging normal weekends. Retain bounded
   local backups (initially seven daily copies), verify restoration on a scratch copy,
   and document that off-host backups are a separate future step.
4. Keep GitHub Actions as YAML invoking Cargo and native Rust executables. Match ADU's
   pinned, validated toolchain and Linux/macOS checks. Use Rust for any new packaging
   or manifest helper; do not copy ADU's Python packaging step. Keep credential-free
   immutable release artifacts, checksum verification, stale-main promotion protection,
   atomic activation, and current/previous release retention.
5. Add an explicit existing-state/read-only check before deployment activation. Preserve
   the baseline, permanent seen IDs, pending payloads, record keys, timestamps and receipts.
   Do not introduce a schema migration for this scheduler change. Refuse incompatible
   future schema changes until a tested migration path exists. Updater and privileged
   unit changes remain an explicit operator install, as with ADU.
6. Test health schedule boundaries, backups and preserved publication state. Run format,
   Clippy and tests; validate job files and systemd units. Update operations/rollback docs.

## Production cutover after approval

1. Recheck active runs and take a consistent SQLite backup plus copies of this bot's
   units and scheduler configuration. Record counts, permissions and active release.
2. Stage the tested release and new service/job definitions. Reuse the existing root-only
   credential file; replace it over SSH only if an authenticated check shows it is invalid
   and the local credential is the intended replacement. Never put secrets in commits,
   release assets, Actions logs, or Petit configuration.
3. Disable this bot's two standalone timers and wait for any in-flight worker/updater to
   finish. Install the start-only authorization rule and jobs. Keep all other bot jobs
   and the shared scheduler concurrency unchanged.
4. Verify whether the installed Petit revision supports safe configuration reload. If a
   restart is needed, use a quiet window, wait for shared jobs to finish, and account for
   the shared service's existing startup recovery/catch-up commands before restarting.
5. Trigger the updater, health and backup jobs through Petit. Perform one authorized
   worker scan through Petit; it may publish genuinely new datasets, never replay the
   baseline. Verify Petit history, systemd exit status, scan timestamp, counts and limits.
6. Verify a successful main release reaches production through the Petit deployment job,
   and confirm only Petit now schedules this bot. Preserve the SQLite state throughout.

## Rollback

Disable/remove only the new bot jobs and reload Petit safely, wait for any in-flight bot
run, then restore the previous worker/deployment timers. Pause automatic deployment before
switching to the previous compatible executable. Preserve the current database and post
receipts; do not restore an older database merely to roll back code.

## Approval boundary

No production files, services, jobs, credentials or data were changed during inspection.
The user approved execution after reviewing this plan. Pushing implementation to main is part of the approved
cutover because the currently active updater automatically installs successful main builds.
