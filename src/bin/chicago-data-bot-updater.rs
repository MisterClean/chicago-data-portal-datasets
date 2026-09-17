use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
    time::Duration,
};

const REPO: &str = "MisterClean/chicago-data-portal-datasets";
const BINARY: &str = "chicago-data-bot-linux-amd64";
const UPDATER: &str = "chicago-data-bot-updater-linux-amd64";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn valid_tag(tag: &str) -> bool {
    tag.strip_prefix("build-").is_some_and(|sha| {
        sha.len() == 40
            && sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn checksum(manifest: &str) -> Result<String> {
    let mut result = None;
    let mut names = std::collections::HashSet::new();
    for line in manifest.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(fields.len() == 2, "Unexpected checksum manifest");
        ensure!(
            fields[0].len() == 64
                && fields[0]
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid SHA256"
        );
        ensure!(
            [BINARY, UPDATER].contains(&fields[1]) && names.insert(fields[1]),
            "Unexpected or repeated asset name"
        );
        if fields[1] == BINARY {
            result = Some(fields[0].to_string());
        }
    }
    result.context("Missing bot checksum")
}
fn download(client: &Client, url: &str, destination: &Path, limit: usize) -> Result<String> {
    let mut response = client.get(url).send()?.error_for_status()?;
    let mut output = File::create(destination)?;
    let mut digest = Sha256::new();
    let mut total = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let length = response.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        total += length;
        ensure!(total <= limit, "Release asset exceeded download limit");
        output.write_all(&buffer[..length])?;
        digest.update(&buffer[..length]);
    }
    output.sync_all()?;
    Ok(format!("{:x}", digest.finalize()))
}
fn file_checksum(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest)?;
    Ok(format!("{:x}", digest.finalize()))
}
fn switch_link(root: &Path, name: &str, target: &Path) -> Result<()> {
    let pending = root.join(format!(".{name}-next"));
    if pending.is_symlink() {
        fs::remove_file(&pending)?;
    }
    symlink(target, &pending)?;
    fs::rename(pending, root.join(name))?;
    File::open(root)?.sync_all()?;
    Ok(())
}
fn install(client: &Client, root: &Path) -> Result<()> {
    let releases = root.join("releases");
    fs::create_dir_all(&releases)?;
    let work = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(&releases)?;
    // Directories must be traversable by the separate runtime user after activation.
    fs::set_permissions(work.path(), fs::Permissions::from_mode(0o755))?;
    let metadata = work.path().join("release.json");
    download(
        client,
        &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        &metadata,
        256_000,
    )?;
    let release: Release = serde_json::from_reader(File::open(&metadata)?)?;
    ensure!(
        valid_tag(&release.tag_name) && !release.draft && !release.prerelease,
        "Latest release is not an eligible production build"
    );
    let target = releases.join(&release.tag_name);
    let current = root.join("current");
    if current.is_symlink() && fs::canonicalize(&current)? == target {
        println!("Already running {}", release.tag_name);
        return Ok(());
    }
    let asset_url = |name: &str| -> Result<String> {
        let expected = format!(
            "https://github.com/{REPO}/releases/download/{}/{name}",
            release.tag_name
        );
        ensure!(
            release
                .assets
                .iter()
                .any(|asset| asset.name == name && asset.browser_download_url == expected),
            "Missing or unexpected release asset: {name}"
        );
        Ok(expected)
    };
    let manifest = work.path().join("SHA256SUMS");
    download(client, &asset_url("SHA256SUMS")?, &manifest, 4096)?;
    let expected = checksum(&fs::read_to_string(manifest)?)?;
    let binary = work.path().join("chicago-data-bot");
    let actual = download(client, &asset_url(BINARY)?, &binary, 32_000_000)?;
    ensure!(
        actual == expected,
        "Release checksum mismatch; active binary unchanged"
    );
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(&binary)
        .arg("--version")
        .current_dir(work.path())
        .output()?;
    ensure!(
        output.status.success(),
        "Downloaded executable failed --version smoke test"
    );
    if target.exists() {
        ensure!(
            file_checksum(&target.join("chicago-data-bot"))? == expected,
            "Existing release directory has unexpected content"
        );
    } else {
        // Rename the whole prepared directory: an interrupted download cannot create a partial release.
        fs::rename(work.path(), &target)?;
        File::open(&releases)?.sync_all()?;
    }
    if current.is_symlink() {
        switch_link(root, "previous", &fs::canonicalize(&current)?)?;
    }
    switch_link(root, "current", &target)?;
    let previous = root.join("previous");
    let previous = if previous.is_symlink() {
        Some(fs::canonicalize(previous)?)
    } else {
        None
    };
    for entry in fs::read_dir(&releases)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && valid_tag(&entry.file_name().to_string_lossy())
            && entry.path() != target
            && Some(entry.path()) != previous
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    println!("Installed {}; SHA256 {expected}", release.tag_name);
    Ok(())
}
fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--version") {
        println!("chicago-data-bot-updater {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if Path::new("/etc/chicago-data-bot/deploy-paused").exists() {
        println!("Deployment paused by operator");
        return Ok(());
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open("/var/lib/chicago-data-deploy/update.lock")?;
    lock.try_lock_exclusive()
        .context("Another deployment is already running")?;
    let client = Client::builder()
        .user_agent("chicago-data-bot-deploy/1")
        .timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(15))
        .build()?;
    install(&client, Path::new("/opt/chicago-data-bot"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_manifests_and_unsafe_release_names() {
        for text in [
            "".into(),
            "bad file".into(),
            format!("{} ../other", "a".repeat(64)),
            format!("{} {BINARY}\n{} {BINARY}", "a".repeat(64), "a".repeat(64)),
        ] {
            assert!(checksum(&text).is_err());
        }
        assert!(
            checksum(&format!(
                "{} {BINARY}\n{} {UPDATER}",
                "a".repeat(64),
                "b".repeat(64)
            ))
            .is_ok()
        );
        assert!(!valid_tag("build-../../elsewhere"));
        assert!(!valid_tag("main"));
        assert!(valid_tag(&format!("build-{}", "a".repeat(40))));
    }
    #[test]
    fn failed_download_does_not_change_active_release() {
        use std::net::TcpListener;
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        fs::create_dir(&old).unwrap();
        switch_link(directory.path(), "current", &old).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            let _ = stream.read(&mut buffer).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\noversize",
                )
                .unwrap();
        });
        assert!(download(&Client::new(), &url, &directory.path().join("download"), 4).is_err());
        thread.join().unwrap();
        assert_eq!(
            fs::canonicalize(directory.path().join("current")).unwrap(),
            fs::canonicalize(old).unwrap()
        );
    }
}
