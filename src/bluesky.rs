use crate::{catalog::Dataset, render};
use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use serde_json::{Value, json};

pub struct Session {
    client: Client,
    pds: String,
    token: String,
    did: String,
}
impl Session {
    pub fn verify(&self) {
        println!(
            "Bluesky app-password authentication succeeded for {}",
            self.did
        );
    }
    pub fn login(client: Client) -> Result<Self> {
        let pds = std::env::var("BSKY_PDS").unwrap_or_else(|_| "https://bsky.social".into());
        let url = reqwest::Url::parse(&pds)?;
        ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "BSKY_PDS must be an HTTPS origin"
        );
        let identifier = std::env::var("BSKY_IDENTIFIER").context("Set BSKY_IDENTIFIER")?;
        let password = std::env::var("BSKY_APP_PASSWORD")
            .context("Set BSKY_APP_PASSWORD to a Bluesky app password")?;
        let response = client
            .post(format!(
                "{}/xrpc/com.atproto.server.createSession",
                pds.trim_end_matches('/')
            ))
            .json(&json!({"identifier":identifier,"password":password}))
            .send()?;
        // Never print authentication response bodies or credentials.
        ensure!(
            response.status().is_success(),
            "Bluesky authentication failed: {}",
            response.status()
        );
        let v: Value = response.json()?;
        Ok(Self {
            client,
            pds: pds.trim_end_matches('/').into(),
            token: v["accessJwt"].as_str().context("Missing token")?.into(),
            did: v["did"].as_str().context("Missing DID")?.into(),
        })
    }
    pub fn publish(&self, d: &Dataset, rkey: &str, created: &str) -> Result<String> {
        let text = render::post_text(d)?;
        let existing = self
            .client
            .get(format!("{}/xrpc/com.atproto.repo.getRecord", self.pds))
            .query(&[
                ("repo", self.did.as_str()),
                ("collection", "app.bsky.feed.post"),
                ("rkey", rkey),
            ])
            .bearer_auth(&self.token)
            .send()?;
        if existing.status().is_success() {
            let value: Value = existing.json()?;
            ensure!(
                value["value"]["text"] == text && value["value"]["createdAt"] == created,
                "Existing record does not match pending post; refusing to overwrite it"
            );
            return Ok(value["uri"].as_str().context("Record missing URI")?.into());
        }
        let status = existing.status();
        let error: Value = existing
            .json()
            .context("Could not check previous publication")?;
        ensure!(
            status == reqwest::StatusCode::BAD_REQUEST && error["error"] == "RecordNotFound",
            "Record lookup failed ({status}); retry later"
        );
        let mut images = Vec::new();
        for card in render::cards(d)? {
            let response = self
                .client
                .post(format!("{}/xrpc/com.atproto.repo.uploadBlob", self.pds))
                .bearer_auth(&self.token)
                .header("Content-Type", "image/png")
                .body(card.png)
                .send()?
                .error_for_status()?;
            let v: Value = response.json()?;
            ensure!(v["blob"].is_object(), "Upload returned no blob");
            images.push(json!({"alt":card.alt,"image":v["blob"],"aspectRatio":{"width":render::WIDTH,"height":card.height}}));
        }
        let url = d.url();
        let start = text.rfind(&url).context("Post missing link")?;
        let record = json!({"$type":"app.bsky.feed.post","text":text,"createdAt":created,"langs":["en"],"facets":[{"index":{"byteStart":start,"byteEnd":start+url.len()},"features":[{"$type":"app.bsky.richtext.facet#link","uri":url}]}],"embed":{"$type":"app.bsky.embed.images","images":images}});
        // A persisted rkey makes retries safe after a response is lost. CAS prevents overwrites.
        let response=self.client.post(format!("{}/xrpc/com.atproto.repo.putRecord",self.pds)).bearer_auth(&self.token).json(&json!({"repo":self.did,"collection":"app.bsky.feed.post","rkey":rkey,"swapRecord":null,"validate":true,"record":record})).send()?;
        if !response.status().is_success() {
            bail!(
                "Bluesky publication failed ({}); pending record retained",
                response.status()
            );
        }
        let v: Value = response.json()?;
        Ok(v["uri"].as_str().context("Publication missing URI")?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn fixture() -> Dataset {
        let p: crate::catalog::Page =
            serde_json::from_str(include_str!("../tests/fixtures/adu.json")).unwrap();
        p.results.into_iter().next().unwrap().dataset().unwrap()
    }
    fn server(replies: Vec<(&'static str, Value)>) -> (Session, thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let pds = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let len: usize = header
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|s| s.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                requests.push(bytes);
                let body = body.to_string();
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        (
            Session {
                client: Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .unwrap(),
                pds,
                token: "test-only".into(),
                did: "did:plc:test".into(),
            },
            handle,
        )
    }
    #[test]
    fn recovered_post_does_not_upload_or_create_again() {
        let d = fixture();
        let text = render::post_text(&d).unwrap();
        let (s, server) = server(vec![(
            "200 OK",
            json!({"uri":"at://existing","value":{"text":text,"createdAt":"time"}}),
        )]);
        assert_eq!(
            s.publish(&d, "3testkey222222", "time").unwrap(),
            "at://existing"
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }
    #[test]
    fn publishes_image_with_alt_link_and_create_only_guard() {
        let (s, server) = server(vec![
            ("400 Bad Request", json!({"error":"RecordNotFound"})),
            (
                "200 OK",
                json!({"blob":{"$type":"blob","ref":{"$link":"test"},"mimeType":"image/png","size":123}}),
            ),
            ("200 OK", json!({"uri":"at://created"})),
        ]);
        let d = fixture();
        assert_eq!(
            s.publish(&d, "3testkey222222", "time").unwrap(),
            "at://created"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        let last = &requests[2];
        let end = last.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let body: Value = serde_json::from_slice(&last[end..]).unwrap();
        assert!(body.as_object().unwrap().contains_key("swapRecord"));
        assert!(body["swapRecord"].is_null());
        assert_eq!(body["rkey"], "3testkey222222");
        assert_eq!(
            body["record"]["embed"]["images"].as_array().unwrap().len(),
            1
        );
        assert!(
            body["record"]["embed"]["images"][0]["alt"]
                .as_str()
                .unwrap()
                .contains("retired")
        );
        let text = body["record"]["text"].as_str().unwrap();
        let index = &body["record"]["facets"][0]["index"];
        assert_eq!(
            &text[index["byteStart"].as_u64().unwrap() as usize
                ..index["byteEnd"].as_u64().unwrap() as usize],
            d.url()
        );
    }
    #[test]
    fn lookup_errors_do_not_trigger_a_write() {
        let (s, server) = server(vec![(
            "503 Service Unavailable",
            json!({"error":"Unavailable"}),
        )]);
        assert!(s.publish(&fixture(), "3testkey222222", "time").is_err());
        assert_eq!(server.join().unwrap().len(), 1);
    }
}
