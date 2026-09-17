# Validation — September 16, 2026 (America/Chicago)

Local environment: macOS arm64, Rust 1.94.0. Linux CI is configured but has not been run remotely.

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

The authenticated Rust API path was tested against a local mock HTTP server. Production credential authentication and remote API publication by the Rust executable still require the bot's app password. A manual browser post tests the generated content and attachment independently; it does not validate the Rust login flow. Deployment and scheduled publishing have not been enabled.

Manual live test: [ADU test post](https://bsky.app/profile/new-chi-data.bsky.social/post/3mvosd6cbtc2j), published through the user-authorized browser session after email verification. The public Bluesky API independently confirmed one attached image and the complete description alt text. The post is explicitly marked TEST and remains available for the user to delete.

## Typography and link polish

The updated renderer passes 13 tests, Clippy with warnings denied, formatting checks, and a release build. The new ADU preview is `preview/polished/description-1.png`; it was visually inspected. Its links use styled labels and compact Chicago URLs, with original destinations intact in alt text. The previous test post has not been replaced.

A fresh release preview measured 0.69 seconds and 45,105,152 bytes maximum RSS (43.02 MiB) on the same Mac. This supersedes the earlier 30,769,152-byte rendering measurement: the new layout loads both regular and real semibold font faces. Catalog scanning code is unchanged. See `DESIGN.md` for applied skill guidance and verification boundaries.
