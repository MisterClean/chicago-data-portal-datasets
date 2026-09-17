# Validation — September 16, 2026 (America/Chicago)

Initial local environment: macOS arm64, Rust 1.94.0. The sections below record the initial checks chronologically; production verification is recorded at the end.

- `cargo test --locked`: 9 passed. Covers live-response fixture extraction, caption limits, readable card size bounds, oversized-description rejection, durable pending records, initial baseline, newly observed IDs, disappearance/reappearance, interrupted/duplicate-page scan rollback, simulated Bluesky upload and create-only write, recovery of an already accepted post, and refusal to publish after a lookup error.
- `cargo clippy --all-targets -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- Live Socrata initialization: 915 official datasets, zero pending announcements.
- Live repeat scan: 915 datasets, zero new announcements.
- Separate copied test database: removed only the ADU ID from its known set, ran two live scans, verified exactly one pending ADU announcement and no publication. Production baseline was not altered by this simulation.
- ADU PNG inspected visually: complete requested metadata, description paragraphs and source link references; 1200 × 1363 pixels, about 165 KiB.

Measured with macOS `/usr/bin/time -l` against the release executable:

| Operation | Wall time | Maximum RSS |
| --- | ---: | ---: |
| ADU fetch + image preview | 1.41 s | 30,769,152 bytes (29.34 MiB) |
| Full initial catalog scan | 2.78 s | 17,481,728 bytes (16.67 MiB) |
| Full repeat scan | 3.78 s | 16,465,920 bytes (15.70 MiB) |

Release executable: about 4.8 MiB. SQLite baseline: 92 KiB. These are one-run local measurements, not memory ceilings or Linux benchmarks. Full authenticated publishing memory has not been measured. No background process remains between scheduled runs.

The authenticated Rust API path was tested against a local mock HTTP server. At that stage, production credential authentication and remote API publication by the Rust executable still required the bot's app password. A manual browser post tests the generated content and attachment independently; it does not validate the Rust login flow. Deployment and scheduled publishing were not yet enabled at that stage.

Manual live test: [ADU test post](https://bsky.app/profile/new-chi-data.bsky.social/post/3mvosd6cbtc2j), published through the user-authorized browser session after email verification. The public Bluesky API independently confirmed one attached image and the complete description alt text. The post is explicitly marked TEST and remains available for the user to delete.

## Typography and link polish

The updated renderer passes 13 tests, Clippy with warnings denied, formatting checks, and a release build. The new ADU preview is `preview/polished/description-1.png`; it was visually inspected. Its links use styled labels and compact Chicago URLs, with original destinations intact in alt text. The previous test post has not been replaced.

A fresh release preview measured 0.69 seconds and 45,105,152 bytes maximum RSS (43.02 MiB) on the same Mac. This supersedes the earlier 30,769,152-byte rendering measurement: the new layout loads both regular and real semibold font faces. Catalog scanning code is unchanged. See `DESIGN.md` for applied skill guidance and verification boundaries.


## Lightsail production — September 16, 2026 (Chicago time)

- Public source: https://github.com/MisterClean/chicago-data-portal-datasets (main).
- Both bot and release updater are Rust executables; there is no production Python dependency.
- GitHub Ubuntu 24.04 CI passed all 15 Rust tests, formatting, Clippy, and release builds. It published immutable Linux amd64 executables and SHA-256 checksums.
- Initial installation used release `build-bf97e589a250e0d5aef46ea1f2d9d5831171c877`. The server's Rust updater fetched and verified the bot from GitHub, smoke-tested it, and activated it atomically.
- Local `.env` is ignored by Git and mode 600. Production `/etc/chicago-data-bot/.env` is root-owned mode 600. Credentials and the Lightsail SSH key are absent from tracked files.
- App-password authentication succeeded locally and from the production binary for the expected bot DID. No password or session token was printed or stored in GitHub.
- Production catalog scan: 915 known IDs, zero pending, service exit 0. The existing baseline was transferred using SQLite's backup API, so deployment did not announce old datasets.
- Polished ADU rendering under the production CPU/memory limits: 0.82 seconds, 32,600 KiB maximum RSS (31.84 MiB), exit 0, no process swaps reported by `/usr/bin/time -v`.
- Runtime service limit: 96 MiB RAM / 32 MiB swap / 25% CPU. Rust updater: 64 MiB RAM / 16 MiB swap / 25% CPU. These are limits, not idle reservations.
- Both timers enabled. Catalog schedule: weekdays 08:00, 14:00, 18:00 America/Chicago, up to five minutes jitter. Release polling: every 15 minutes, up to two minutes jitter.
- Verified the updater user cannot read bot credentials and the runtime user cannot modify release binaries. Existing bots were not reconfigured or restarted. A pre-existing `divvy-bot-health.service` failure was observed and left untouched.

The scheduled scan used `--publish` but had no new datasets, so it sent no announcement. Authentication and remote scanning/rendering are verified; the Rust write path remains covered by mock-server tests until the first new announcement. The earlier manually published browser test is unchanged.
