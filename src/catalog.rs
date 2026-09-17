use anyhow::{Result, ensure};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub const DOMAIN: &str = "data.cityofchicago.org";
const API: &str = "https://api.us.socrata.com/api/catalog/v1";

#[derive(Debug, Deserialize)]
pub struct Page {
    pub results: Vec<Entry>,
    #[serde(rename = "resultSetSize")]
    pub total: usize,
    #[serde(default)]
    pub warnings: Vec<serde_json::Value>,
}
#[derive(Debug, Deserialize)]
pub struct Entry {
    pub resource: Resource,
    pub classification: Classification,
    pub owner: Option<Owner>,
    pub metadata: Metadata,
}
#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub domain: String,
}
#[derive(Debug, Deserialize)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub provenance: String,
}
#[derive(Debug, Deserialize)]
pub struct Classification {
    pub domain_category: Option<String>,
    #[serde(default)]
    pub domain_metadata: Vec<Field>,
}
#[derive(Debug, Deserialize)]
pub struct Field {
    pub key: String,
    pub value: String,
}
#[derive(Debug, Deserialize)]
pub struct Owner {
    pub display_name: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dataset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub category: String,
    pub data_owner: String,
    pub dataset_owner: String,
}
pub fn valid_id(id: &str) -> bool {
    id.len() == 9
        && id.as_bytes()[4] == b'-'
        && id
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_lowercase() || b.is_ascii_digit())
}
impl Entry {
    pub fn dataset(self) -> Result<Dataset> {
        ensure!(
            self.metadata.domain == DOMAIN
                && self.resource.kind == "dataset"
                && self.resource.provenance == "official",
            "Unexpected catalog scope"
        );
        ensure!(valid_id(&self.resource.id), "Invalid dataset ID");
        let fallback = || "Not provided".to_string();
        Ok(Dataset {
            id: self.resource.id,
            name: self.resource.name,
            description: self
                .resource
                .description
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "No description provided.".into()),
            created_at: self.resource.created_at,
            category: self.classification.domain_category.unwrap_or_else(fallback),
            data_owner: self
                .classification
                .domain_metadata
                .into_iter()
                .find(|f| f.key == "Metadata_Data-Owner")
                .map(|f| f.value)
                .unwrap_or_else(fallback),
            dataset_owner: self
                .owner
                .and_then(|o| o.display_name)
                .unwrap_or_else(fallback),
        })
    }
}
impl Dataset {
    pub fn url(&self) -> String {
        format!("https://{DOMAIN}/d/{}", self.id)
    }
    pub fn date(&self) -> Result<String> {
        Ok(chrono::DateTime::parse_from_rfc3339(&self.created_at)?
            .with_timezone(&chrono_tz::America::Chicago)
            .format("%B %-d, %Y")
            .to_string())
    }
}
pub fn page(client: &Client, offset: usize, id: Option<&str>) -> Result<Page> {
    let mut params = vec![
        ("domains", DOMAIN.to_string()),
        ("only", "dataset".into()),
        ("provenance", "official".into()),
        ("limit", "100".into()),
        ("offset", offset.to_string()),
        ("order", "name".into()),
    ];
    if let Some(id) = id {
        ensure!(valid_id(id), "Expected a Socrata ID such as j4h8-ug9m");
        params.push(("ids", id.into()));
    }
    let response = client.get(API).query(&params).send()?.error_for_status()?;
    let mut bytes = Vec::new();
    response.take(8_000_001).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 8_000_000,
        "Catalog page exceeds 8 MB safety limit"
    );
    let page: Page = serde_json::from_slice(&bytes)?;
    ensure!(
        page.warnings.is_empty(),
        "Catalog returned warnings: {:?}",
        page.warnings
    );
    Ok(page)
}
