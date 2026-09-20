use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc, Weekday};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

fn open(path: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(std::time::Duration::from_secs(10))?;
    Ok(db)
}
fn verify(db: &Connection) -> Result<()> {
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Database integrity check failed");
    let baseline: String =
        db.query_row("SELECT value FROM meta WHERE key='baseline'", [], |r| {
            r.get(0)
        })?;
    DateTime::parse_from_rfc3339(&baseline).context("Invalid baseline timestamp")?;
    db.prepare("SELECT id,first_seen FROM seen")?;
    db.prepare("SELECT id,payload,rkey,created_at,uri FROM outbox")?;
    Ok(())
}
// Allow 30 minutes for the scheduled worker to complete; weekends are intentional.
fn due(now: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let tz = chrono_tz::America::Chicago;
    let cutoff = now - Duration::minutes(30);
    let date = cutoff.with_timezone(&tz).date_naive();
    for back in 0..8 {
        let day = date - Duration::days(back);
        if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            continue;
        }
        for hour in [18, 14, 8] {
            let local = day.and_hms_opt(hour, 3, 0).context("Invalid schedule")?;
            let candidate = tz
                .from_local_datetime(&local)
                .single()
                .context("Ambiguous schedule")?
                .with_timezone(&Utc);
            if candidate <= cutoff {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("No previous scheduled run")
}
pub fn check(path: &Path, health: bool) -> Result<()> {
    let db = open(path)?;
    verify(&db)?;
    if health {
        let last: String =
            db.query_row("SELECT value FROM meta WHERE key='last_scan'", [], |r| {
                r.get(0)
            })?;
        let last = DateTime::parse_from_rfc3339(&last)?.with_timezone(&Utc);
        let expected = due(Utc::now())?;
        ensure!(
            last >= expected,
            "Scheduled scan overdue: last successful scan {last}"
        );
        let oldest: Option<String> = db.query_row(
            "SELECT min(created_at) FROM outbox WHERE uri IS NULL",
            [],
            |r| r.get(0),
        )?;
        if let Some(oldest) = oldest {
            ensure!(
                DateTime::parse_from_rfc3339(&oldest)?.with_timezone(&Utc) >= expected,
                "Pending announcements survived a scheduled run"
            );
        }
    }
    println!(
        "Database check passed{}",
        if health {
            " (including schedule and queue health)"
        } else {
            ""
        }
    );
    Ok(())
}
pub fn backup(path: &Path, output: &Path) -> Result<()> {
    let source = open(path)?;
    verify(&source)?;
    std::fs::create_dir_all(output)?;
    let temp = tempfile::NamedTempFile::new_in(output)?;
    {
        let mut dest = Connection::open(temp.path())?;
        rusqlite::backup::Backup::new(&source, &mut dest)?.run_to_completion(
            64,
            std::time::Duration::from_millis(20),
            None,
        )?;
        verify(&dest)?;
    }
    temp.as_file().sync_all()?;
    let name = format!(
        "catalog-backup-{}.sqlite",
        Utc::now().format("%Y%m%dT%H%M%S%.6fZ")
    );
    let target = output.join(name);
    temp.persist_noclobber(&target)?;
    let mut backups: Vec<_> = std::fs::read_dir(output)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("catalog-backup-")
                && name.ends_with(".sqlite")
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .map(|e| e.path())
        .collect();
    backups.sort();
    for old in backups.iter().take(backups.len().saturating_sub(7)) {
        std::fs::remove_file(old)?;
    }
    println!("Verified backup: {}", target.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weekends_and_dst_follow_chicago_schedule() {
        for (now, expected) in [
            ("2026-09-20T14:00:00Z", "2026-09-18T23:03:00Z"),
            ("2026-09-21T13:20:00Z", "2026-09-18T23:03:00Z"),
            ("2026-09-21T13:34:00Z", "2026-09-21T13:03:00Z"),
            ("2026-11-02T14:34:00Z", "2026-11-02T14:03:00Z"),
        ] {
            assert_eq!(
                due(now.parse().unwrap()).unwrap(),
                expected.parse::<DateTime<Utc>>().unwrap()
            );
        }
    }
    #[test]
    fn backup_preserves_receipts_and_missing_check_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.sqlite");
        assert!(check(&path, false).is_err());
        assert!(!path.exists());
        let db = crate::state::open(&path).unwrap();
        db.execute(
            "INSERT INTO meta VALUES('baseline','2026-09-17T00:00:00Z')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO seen VALUES('abcd-1234','time')", [])
            .unwrap();
        db.execute(
            "INSERT INTO outbox VALUES('abcd-1234','{}','key','time','at://receipt')",
            [],
        )
        .unwrap();
        let output = dir.path().join("backups");
        backup(&path, &output).unwrap();
        let file = std::fs::read_dir(output)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let restored = open(&file).unwrap();
        assert_eq!(
            restored
                .query_row("SELECT uri FROM outbox", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "at://receipt"
        );
        assert_eq!(
            restored
                .query_row("SELECT count(*) FROM seen", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
