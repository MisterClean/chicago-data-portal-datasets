# Lightsail production operations

Ubuntu 24.04 amd64. The bot runs directly as a small Rust executable, alongside the existing bots. Builds happen on GitHub-hosted runners.

## Installed paths and ownership

| Path | Purpose |
| --- | --- |
| `/etc/chicago-data-bot/.env` | Root-owned mode 600 app password; systemd reads it before switching users |
| `/var/lib/chicago-data-bot/catalog.sqlite` | Durable dataset history and pending posts; owned by `chicago-data-bot` |
| `/opt/chicago-data-bot/releases/build-COMMIT/` | Checked Linux executables; owned by `chicago-data-deploy` |
| `/opt/chicago-data-bot/current` | Atomically selected active release |
| `/opt/chicago-data-bot/previous` | Previous release, retained for rollback |
| `/usr/local/libexec/chicago-data-bot-update` | Root-owned Rust updater |
| `/var/lib/chicago-data-deploy/` | Updater lock/state, isolated from bot data and credentials |

The runtime user can write only its state directory. The deployment user can update only this bot's release directories and has no access to the app password. No GitHub credentials or SSH keys are needed for pull deployment from the public repository. Checksums detect corruption; authenticity depends on HTTPS and control of the GitHub repository/release workflow.

Runtime limits: 96 MiB RAM, 32 MiB swap, 25% CPU, 32 tasks; lower scheduling priority. Updater limits: 64 MiB RAM, 16 MiB swap, 25% CPU. Both exit after work. No existing Docker/PM2 bot is restarted or reconfigured.

## Schedule

`chicago-data-bot.timer`: weekdays at **08:00, 14:00, 18:00 America/Chicago**, with 0–5 minutes of jitter. Systemd handles daylight saving time. `Persistent=false` avoids boot-time catch-up outside working hours. Friday evening to Monday morning is the longest detection delay; use a daily or hourly schedule if weekend announcements become desirable.

`chicago-data-bot-deploy.timer`: checks releases every 15 minutes, with up to two minutes of jitter. Updating a binary never resets the SQLite baseline or triggers a post.

## CI/CD

Push to `main` → tests/format/lint → release build on Ubuntu 24.04 → immutable `build-FULL_COMMIT_SHA` release with binary + checksum → host downloads, verifies, smoke-tests `--version`, then atomically switches `current`. The publishing job checks that its commit is still the head of main before promotion. Fork PRs run checks with read-only permissions and cannot publish. Failed CI leaves the current release installed.

Keep release assets immutable. Use a new main commit for fixes. A rerun of an already released SHA does not overwrite its assets. Workflow and unit configuration changes are reviewed/applied separately: the pull updater intentionally updates only the executable, not privileged systemd units or its own script.

## Inspect and run

```sh
systemctl list-timers 'chicago-data-bot*'
systemctl status chicago-data-bot.service chicago-data-bot-deploy.service
journalctl -u chicago-data-bot.service -u chicago-data-bot-deploy.service --since today
readlink -f /opt/chicago-data-bot/current
sudo -u chicago-data-bot /opt/chicago-data-bot/current/chicago-data-bot --state /var/lib/chicago-data-bot/catalog.sqlite status
sudo systemctl start chicago-data-bot-deploy.service
sudo systemctl start chicago-data-bot.service
```

A successful oneshot service is normally `inactive (dead)` between runs, with `Result=success`. On failure, inspect its journal; the next scheduled invocation retries. Pending posts are retained on failure. Systemd failures are visible in `systemctl --failed`; no external alert destination is configured.

## Roll back

Stop updates first so the next poll does not reinstall the problematic release:

```sh
sudo systemctl stop chicago-data-bot-deploy.timer
sudo systemctl stop chicago-data-bot-deploy.service
sudo touch /etc/chicago-data-bot/deploy-paused
sudo systemctl stop chicago-data-bot.timer
sudo systemctl stop chicago-data-bot.service
sudo -u chicago-data-deploy ln -sfn "$(readlink -f /opt/chicago-data-bot/previous)" /opt/chicago-data-bot/.rollback
sudo -u chicago-data-deploy mv -Tf /opt/chicago-data-bot/.rollback /opt/chicago-data-bot/current
sudo systemctl start chicago-data-bot.timer
```

Verify `previous` exists before these commands. After a fixed release is available, remove only `/etc/chicago-data-bot/deploy-paused` and restart the deploy timer. The updater retains active and previous release directories, pruning older ones. Future schema migrations must remain compatible with rollback or require a database backup/restore plan.

## Fresh host bootstrap

Create system users `chicago-data-bot` and `chicago-data-deploy` without interactive login; prepare the directories above with matching ownership. Install the `.service` and `.timer` files in `/etc/systemd/system`, and the verified `chicago-data-bot-updater-linux-amd64` release executable as `/usr/local/libexec/chicago-data-bot-update`. Install `.env` separately with mode 600; never transfer it via GitHub.

Run `systemctl daemon-reload`, start the deploy service, and initialize the database once as `chicago-data-bot` using `current/chicago-data-bot --state /var/lib/chicago-data-bot/catalog.sqlite init`. Then enable both timers. To preserve detection continuity when moving hosts, transfer the existing SQLite database using its backup API instead of taking a fresh baseline. Do not copy a live database without accounting for its WAL file.
