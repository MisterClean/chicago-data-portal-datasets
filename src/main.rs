mod bluesky;
mod catalog;
mod render;
mod state;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use reqwest::blocking::Client;
use std::{fs, fs::OpenOptions, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    about = "Announce newly discovered official Chicago datasets on Bluesky"
)]
struct Args {
    #[arg(
        long,
        env = "BOT_STATE",
        default_value = "state/catalog.sqlite",
        global = true
    )]
    state: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Establish a baseline without posting. Run exactly once before scheduling.
    Init,
    /// Verify app-password authentication without posting.
    AuthCheck,
    /// Scan and queue new IDs; only publish when --publish is explicitly supplied.
    Run {
        #[arg(long)]
        publish: bool,
        #[arg(long, default_value_t = 5)]
        max_posts: usize,
    },
    /// Render one real dataset locally without changing state or posting.
    Preview {
        id: String,
        #[arg(long, default_value = "preview")]
        output: PathBuf,
    },
    /// Show baseline, known-ID count, and pending announcements.
    Status,
}
fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent("chicago-data-bot/0.1 (+https://data.cityofchicago.org)")
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
fn main() -> Result<()> {
    match dotenvy::from_path(".env") {
        Ok(_) => {}
        Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => anyhow::bail!("Could not load .env; check its permissions and syntax"),
    }
    let args = Args::parse();
    let client = client()?;
    if matches!(args.command, Command::AuthCheck) {
        bluesky::Session::login(client)?.verify();
        return Ok(());
    }
    if let Command::Preview { id, output } = args.command {
        let mut page = catalog::page(&client, 0, Some(&id))?;
        ensure!(
            page.results.len() == 1,
            "Dataset not found or not an official dataset"
        );
        let d = page.results.remove(0).dataset()?;
        fs::create_dir_all(&output)?;
        fs::write(output.join("post.txt"), render::post_text(&d)?)?;
        fs::write(output.join("dataset.json"), serde_json::to_vec_pretty(&d)?)?;
        for (i, card) in render::cards(&d)?.into_iter().enumerate() {
            fs::write(output.join(format!("description-{}.png", i + 1)), card.png)?;
            fs::write(
                output.join(format!("description-{}.alt.txt", i + 1)),
                card.alt,
            )?;
        }
        println!("Preview saved in {}", output.display());
        return Ok(());
    }
    if let Some(parent) = args.state.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(args.state.with_extension("lockfile"))?;
    lock.try_lock_exclusive()
        .context("Another bot process is using this state database")?;
    let mut db = state::open(&args.state)?;
    match args.command {
        Command::Init => {
            state::scan(&mut db, &client, true)?;
        }
        Command::Run { publish, max_posts } => {
            state::scan(&mut db, &client, false)?;
            if publish && state::next(&db)?.is_some() {
                let session = bluesky::Session::login(client)?;
                for _ in 0..max_posts {
                    let Some((d, rkey, created)) = state::next(&db)? else {
                        break;
                    };
                    let uri = session
                        .publish(&d, &rkey, &created)
                        .with_context(|| format!("Dataset {} remains pending", d.id))?;
                    db.execute(
                        "UPDATE outbox SET uri=?1 WHERE id=?2",
                        rusqlite::params![uri, d.id],
                    )?;
                    println!("Posted {}: {}", d.id, uri);
                }
            } else if !publish {
                println!("Dry run: pending announcements retained; no posts sent.");
            }
        }
        Command::Status => {
            let seen: i64 = db.query_row("SELECT count(*) FROM seen", [], |r| r.get(0))?;
            let pending: i64 =
                db.query_row("SELECT count(*) FROM outbox WHERE uri IS NULL", [], |r| {
                    r.get(0)
                })?;
            println!(
                "Baseline: {}; known IDs: {seen}; pending: {pending}",
                state::initialized(&db)?
            );
        }
        Command::Preview { .. } | Command::AuthCheck => unreachable!(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;
    fn adu() -> catalog::Dataset {
        let p: catalog::Page =
            serde_json::from_str(include_str!("../tests/fixtures/adu.json")).unwrap();
        p.results.into_iter().next().unwrap().dataset().unwrap()
    }
    #[test]
    fn adu_metadata_and_card() {
        let d = adu();
        assert_eq!(d.data_owner, "Department of Housing");
        assert_eq!(d.dataset_owner, "Maria Fiorillo");
        assert_eq!(d.date().unwrap(), "May 13, 2026");
        let text = render::post_text(&d).unwrap();
        assert!(text.graphemes(true).count() <= 300);
        assert!(text.contains("Dataset Owner: Maria Fiorillo"));
        let cards = render::cards(&d).unwrap();
        assert!(!cards.is_empty());
        assert!(cards.len() <= 4);
        let all = cards.iter().map(|c| c.alt.as_str()).collect::<String>();
        assert!(all.contains("retired"));
        assert!(all.contains("xbwc-ntpx"));
        assert!(all.contains("chicago.gov/ADU"));
        for card in cards {
            assert!(card.png.len() < 2_000_000);
            let decoder = png::Decoder::new(std::io::Cursor::new(card.png));
            let reader = decoder.read_info().unwrap();
            assert_eq!(reader.info().width, 1200);
        }
    }
    #[test]
    fn huge_metadata_stays_within_post_limit() {
        let mut d = adu();
        d.name = "🏙️ Chicago ".repeat(200);
        d.data_owner = "Long owner ".repeat(80);
        let s = render::post_text(&d).unwrap();
        assert!(s.graphemes(true).count() <= 300);
        assert!(s.ends_with(&d.url()));
    }
    #[test]
    fn oversize_description_is_not_silently_truncated() {
        let mut d = adu();
        d.description = "Very long description. ".repeat(3000);
        assert!(render::cards(&d).is_err());
    }
    #[test]
    fn pending_survives_reopen() {
        let path =
            std::env::temp_dir().join(format!("chicago-bot-test-{}.sqlite", std::process::id()));
        {
            let db = state::open(&path).unwrap();
            let d = adu();
            db.execute(
                "INSERT INTO outbox VALUES(?1,?2,'3testkey222222','2026-09-17T00:00:00Z',NULL)",
                rusqlite::params![d.id, serde_json::to_string(&d).unwrap()],
            )
            .unwrap();
        }
        {
            let db = state::open(&path).unwrap();
            let (d, key, _) = state::next(&db).unwrap().unwrap();
            assert_eq!(d.id, "j4h8-ug9m");
            assert_eq!(key, "3testkey222222");
            db.execute("UPDATE outbox SET uri='at://test'", []).unwrap();
            assert!(state::next(&db).unwrap().is_none());
        }
        fs::remove_file(path).unwrap();
    }
}
