use crate::catalog::{self, Dataset};
use anyhow::{Result, ensure};
use reqwest::blocking::Client;
use rusqlite::{Connection, params};

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-1024;
        CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS seen(id TEXT PRIMARY KEY,first_seen TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS outbox(id TEXT PRIMARY KEY,payload TEXT NOT NULL,rkey TEXT NOT NULL UNIQUE,created_at TEXT NOT NULL,uri TEXT);")?;
    Ok(db)
}
pub fn initialized(db: &Connection) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM meta WHERE key='baseline')",
        [],
        |r| r.get(0),
    )?)
}
pub fn scan(db: &mut Connection, client: &Client, baseline: bool) -> Result<usize> {
    scan_with(db, baseline, |offset| catalog::page(client, offset, None))
}

fn scan_with(
    db: &mut Connection,
    baseline: bool,
    mut fetch: impl FnMut(usize) -> Result<catalog::Page>,
) -> Result<usize> {
    ensure!(
        baseline != initialized(db)?,
        if baseline {
            "Baseline already exists"
        } else {
            "Run init first; refusing to announce the entire catalog"
        }
    );
    let tx = db.transaction()?;
    tx.execute_batch("CREATE TEMP TABLE scan(id TEXT PRIMARY KEY,payload TEXT NOT NULL);")?;
    let mut offset = 0;
    let mut expected = None;
    loop {
        let page = fetch(offset)?;
        ensure!(
            page.total > 0 && page.total <= 100_000,
            "Unexpected catalog size"
        );
        if let Some(n) = expected {
            ensure!(
                n == page.total,
                "Catalog changed during scan; retry next run"
            );
        } else {
            expected = Some(page.total);
        }
        let len = page.results.len();
        ensure!(len > 0, "Catalog ended prematurely");
        for entry in page.results {
            let d = entry.dataset()?;
            tx.execute(
                "INSERT INTO scan VALUES(?1,?2)",
                params![d.id, serde_json::to_string(&d)?],
            )?;
        }
        offset += len;
        ensure!(offset <= page.total, "Catalog pagination mismatch");
        if offset == page.total {
            break;
        }
    }
    // Do not remove old IDs: disappearance/reappearance must not create an announcement.
    let now = chrono::Utc::now().to_rfc3339();
    let mut added = 0;
    if !baseline {
        let mut stmt = tx.prepare(
            "SELECT id,payload FROM scan WHERE id NOT IN (SELECT id FROM seen) ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut micros = chrono::Utc::now().timestamp_micros() as u64;
        for row in rows {
            let (id, payload) = row?;
            micros += 1;
            tx.execute(
                "INSERT INTO outbox(id,payload,rkey,created_at) VALUES(?1,?2,?3,?4)",
                params![id, payload, tid(micros), now],
            )?;
            added += 1;
        }
    }
    tx.execute("INSERT OR IGNORE INTO seen SELECT id,?1 FROM scan", [&now])?;
    tx.execute("INSERT OR REPLACE INTO meta VALUES('last_scan',?1)", [&now])?;
    if baseline {
        tx.execute("INSERT INTO meta VALUES('baseline',?1)", [&now])?;
    }
    tx.execute_batch("DROP TABLE scan;")?;
    tx.commit()?;
    println!("Scanned {offset} official datasets; queued {added} announcements.");
    Ok(added)
}
fn tid(micros: u64) -> String {
    let alphabet = b"234567abcdefghijklmnopqrstuvwxyz";
    let mut n = micros << 10;
    let mut out = [b'2'; 13];
    for i in (0..13).rev() {
        out[i] = alphabet[(n & 31) as usize];
        n >>= 5;
    }
    String::from_utf8(out.to_vec()).unwrap()
}
pub fn next(db: &Connection) -> Result<Option<(Dataset, String, String)>> {
    let mut stmt=db.prepare("SELECT payload,rkey,created_at FROM outbox WHERE uri IS NULL ORDER BY created_at,id LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let payload: String = row.get(0)?;
        Ok(Some((
            serde_json::from_str(&payload)?,
            row.get(1)?,
            row.get(2)?,
        )))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(ids: &[&str]) -> catalog::Page {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/adu.json")).unwrap();
        let entries: Vec<_> = ids
            .iter()
            .map(|id| {
                let mut v = fixture["results"][0].clone();
                v["resource"]["id"] = (*id).into();
                v
            })
            .collect();
        serde_json::from_value(serde_json::json!({"results":entries,"resultSetSize":ids.len()}))
            .unwrap()
    }
    #[test]
    fn baseline_new_ids_and_reappearance() {
        let mut db = open(std::path::Path::new(":memory:")).unwrap();
        assert!(scan_with(&mut db, false, |_| Ok(page(&["aaaa-bbbb"]))).is_err());
        assert_eq!(
            scan_with(&mut db, true, |_| Ok(page(&["aaaa-bbbb"]))).unwrap(),
            0
        );
        assert!(next(&db).unwrap().is_none());
        assert!(scan_with(&mut db, true, |_| Ok(page(&["aaaa-bbbb"]))).is_err());
        assert_eq!(
            scan_with(&mut db, false, |_| Ok(page(&["aaaa-bbbb", "j4h8-ug9m"]))).unwrap(),
            1
        );
        let (_, key, _) = next(&db).unwrap().unwrap();
        assert_eq!(
            scan_with(&mut db, false, |_| Ok(page(&["aaaa-bbbb"]))).unwrap(),
            0
        );
        assert_eq!(
            scan_with(&mut db, false, |_| Ok(page(&["aaaa-bbbb", "j4h8-ug9m"]))).unwrap(),
            0
        );
        assert_eq!(next(&db).unwrap().unwrap().1, key);
    }
    #[test]
    fn partial_scan_does_not_advance_baseline_or_queue() {
        let mut db = open(std::path::Path::new(":memory:")).unwrap();
        let failed = scan_with(&mut db, true, |offset| {
            if offset == 0 {
                let mut p = page(&["aaaa-bbbb"]);
                p.total = 2;
                Ok(p)
            } else {
                anyhow::bail!("network unavailable")
            }
        });
        assert!(failed.is_err());
        assert!(!initialized(&db).unwrap());
        let count: i64 = db
            .query_row("SELECT count(*) FROM seen", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        scan_with(&mut db, true, |_| Ok(page(&["aaaa-bbbb"]))).unwrap();
        assert!(scan_with(&mut db, false, |_| Ok(page(&["j4h8-ug9m", "j4h8-ug9m"]))).is_err());
        assert!(next(&db).unwrap().is_none());
        assert_eq!(
            scan_with(&mut db, false, |_| Ok(page(&["aaaa-bbbb", "j4h8-ug9m"]))).unwrap(),
            1
        );
    }
}
