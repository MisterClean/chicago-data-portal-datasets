# Chicago Data Portal → Bluesky

A small Rust command-line bot that announces **newly discovered official datasets** on Chicago's public Socrata catalog. One short-lived process per scheduled run; SQLite on disk; native PNG text rendering with a bundled, OFL-licensed Noto Sans font. No dataset rows, browser, Python, or separate database server in production.

Posts contain the dataset name, Data Owner, Category, Date Created (America/Chicago), Dataset Owner, and a clickable canonical dataset link. Description images include these same details and the description, with HTML link labels rendered in blue with underlines. Bare Chicago dataset URLs use compact canonical paths; full original destinations are preserved in alt text, without inserting line breaks inside URLs. Image links are visual references, not clickable PNG regions; the post contains the clickable dataset link. Each image includes its visible text as alt text. If metadata cannot fit within Bluesky's 300 graphemes / 3,000 UTF-8 bytes, the caption points to the image for details. Descriptions can span up to four readable cards; oversized content fails visibly and remains pending for manual review, rather than being silently discarded.

## Start locally

Requires Rust/Cargo and a C toolchain to build bundled SQLite. Build on the target platform; the macOS executable does not run on Linux.

```sh
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets -- -D warnings
./target/release/chicago-data-bot preview j4h8-ug9m
./target/release/chicago-data-bot init
./target/release/chicago-data-bot run
./target/release/chicago-data-bot status
```

`preview` only writes local files. `init` establishes the existing catalog baseline and **does not post**. `run` scans and queues new datasets, but does not publish unless `--publish` is present. It is not a read-only preview: queued datasets persist for a subsequent publishing run. Repeating `init` is rejected. State defaults to `state/catalog.sqlite`; use `--state PATH` or `BOT_STATE` to override it.

For automated publishing, supply the account handle, a Bluesky **app password**, and optionally the account's PDS origin through the environment; see `.env.example`. The program loads `.env` from the current working directory, while existing environment variables take precedence. A malformed or unreadable file causes an error. Never use the primary account password, commit credentials, or put them in command-line arguments. Tokens stay in process memory and are not written to disk.

```sh
./target/release/chicago-data-bot run --publish --max-posts 5
```

The browser's existing login can be used for a manual test post, but does not authenticate this standalone bot. On a custom PDS, set `BSKY_PDS` explicitly. Authentication is performed only if pending posts exist. Each execution sends at most five posts by default; HTTP/auth/rate-limit errors exit nonzero and leave the queue intact for the next scheduled run.

## Detection and snapshots

The [Socrata Discovery API](https://socratadiscovery.docs.apiary.io/) is queried at:

```
https://api.us.socrata.com/api/catalog/v1?domains=data.cityofchicago.org&only=dataset&provenance=official&limit=100&offset=0&order=name
```

Only catalog metadata is fetched, in 100-result pages with an 8 MB page safety limit. Large schema arrays returned by Socrata are ignored during deserialization. The bot does not download the datasets themselves. SQLite stages one catalog scan in a transaction, checks counts, unique IDs, scope and warnings, then atomically adds unseen IDs and their frozen announcement payloads. Incomplete or suspicious scans roll back. The catalog is not a transactional snapshot: simultaneous catalog changes can still cause a temporarily missed ID; the next full scan catches it. Name sorting plus duplicate/count checks reduces pagination drift but cannot eliminate it.

Keep one permanent set of IDs, rather than a new JSON snapshot every hour:

- `seen`: ID and first observation time; never delete IDs that disappear from the catalog.
- `outbox`: frozen metadata, publication timestamp, persisted AT Protocol TID, and eventual post URI.
- `meta`: baseline and last successful scan timestamps.

A reappearing dataset with the same ID is not announced again. A replacement dataset with a new ID is announced, even if its title is identical. Existing row updates or description edits do not produce posts. Maps, charts, stories, files and community datasets are excluded from this initial scope.

Creation dates are **display metadata, not a detection watermark**. The [ADU example](https://data.cityofchicago.org/d/j4h8-ug9m) reports `createdAt=2026-05-13T15:42:14Z`, but `publication_date=2026-09-03T22:55:06Z` in the catalog inspected on September 16, 2026 (Chicago time). Old drafts can become public later. The bot therefore compares IDs regardless of age. Its practical meaning of “new” is “first observed in this public catalog after the baseline,” subject to Socrata indexing delay and the polling interval. It cannot recover a dataset that appears and disappears entirely between polls.

Field mapping (confirmed against the ADU catalog entry and `/api/views/j4h8-ug9m.json`):

| Requested field | Catalog field |
| --- | --- |
| Name | `resource.name` |
| Description | `resource.description` |
| Data Owner | `classification.domain_metadata[key=Metadata_Data-Owner].value` |
| Category | `classification.domain_category` |
| Date Created | `resource.createdAt` |
| Dataset Owner | `owner.display_name` |

The view metadata endpoint is useful for inspecting extra fields, but not necessary per dataset: the catalog already supplies all required values. Missing optional metadata is explicitly labeled “Not provided.”

## Reliable publication

The pending payload, timestamp and record key are committed before posting. Each attempt first calls `com.atproto.repo.getRecord`; an existing matching record is treated as successful. Otherwise it renders images, uploads blobs, and calls `com.atproto.repo.putRecord` with the same key and `swapRecord: null` to prevent replacement. This handles a crash or lost response after server acceptance without creating a second post. Failed uploads can leave unreferenced blobs; retries can upload them again. Only a confirmed record URI marks an outbox row complete. Do not delete or regenerate pending keys to retry a failed post.

A process lock prevents overlapping runs against the same state path. Use exactly one state database and publisher per account, and preserve it across deployments. Never replace production state with a fresh baseline: that loses history and pending work. Back up a live database using SQLite's backup API / `.backup`, or stop the service before copying it (including any WAL sidecars).

See the official [posting guide](https://docs.bsky.app/docs/tutorials/creating-a-post), [putRecord API](https://docs.bsky.app/docs/api/com-atproto-repo-put-record), and current [image lexicon](https://github.com/bluesky-social/atproto/blob/main/lexicons/app/bsky/embed/images.json). The renderer enforces four images and less than 2,000,000 bytes per image, and creates UTF-8 byte offsets for the link facet.

## Production and continuous deployment

See [deploy/README.md](deploy/README.md) for the Lightsail installation, schedules, limits, updates, and rollback.

GitHub Actions runs Rust tests, formatting, Clippy and Rust updater checks on pull requests. Successful current `main` builds publish immutable Linux amd64 GitHub releases with SHA-256 checksums. Lightsail polls for the latest successful release every 15 minutes and atomically switches to its prebuilt executable. Failed CI never publishes a release. No Rust compiler, GitHub token, or inbound CI SSH connection is required on the host.

The bot checks for new datasets at **08:00, 14:00, and 18:00 America/Chicago, Monday–Friday**, with up to five minutes of jitter. Missed runs are not replayed overnight; the next business-hours scan catches up. SQLite IDs and pending posts persist across every binary deployment.

`auth-check` validates the configured app password without posting:

```sh
./target/release/chicago-data-bot auth-check
```

Between runs the bot uses no resident memory. Peak memory includes one metadata response, font rasterizers, one RGB card canvas and compressed cards. Local measurements and production verification are recorded in `VALIDATION.md`.

## Shared scheduler

Production runs through Petit, which starts isolated systemd services. See
[Petit operations](deploy/PETIT.md) for the worker, deployment, health and backup
schedules. Rust CI runs on Linux and macOS before publishing a Linux executable.
