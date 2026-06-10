//! Notion API client boundary.
//!
//! The connector depends on this trait rather than directly on HTTP so tests
//! can run against deterministic fixtures and live API calls stay isolated.

use afs_core::{AfsError, AfsResult};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;

use crate::NotionConfig;
use crate::dto::{BlockListDto, PageDto};

pub const DEFAULT_NOTION_API_BASE_URL: &str = "https://api.notion.com";
pub const DEFAULT_NOTION_VERSION: &str = "2026-03-11";
pub const DEFAULT_NOTION_TOKEN_ENV: &str = "NOTION_TOKEN";

pub trait NotionApi: std::fmt::Debug + Send + Sync {
    fn retrieve_page(&self, page_id: &str) -> AfsResult<PageDto>;
    fn retrieve_block_children(
        &self,
        block_id: &str,
        start_cursor: Option<&str>,
    ) -> AfsResult<BlockListDto>;
}

#[derive(Clone, Debug)]
pub struct HttpNotionApi {
    config: NotionConfig,
    client: Client,
}

impl HttpNotionApi {
    pub fn new(config: NotionConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    fn get_json<T>(&self, path: &str, query: &[(&str, String)]) -> AfsResult<T>
    where
        T: DeserializeOwned,
    {
        let token = self.token()?;
        let url = format!(
            "{}/{}",
            DEFAULT_NOTION_API_BASE_URL,
            path.trim_start_matches('/')
        );
        let mut request = self
            .client
            .get(url)
            .bearer_auth(token)
            .header("Notion-Version", DEFAULT_NOTION_VERSION);

        for (key, value) in query {
            request = request.query(&[(key, value)]);
        }

        let response = request
            .send()
            .map_err(|error| AfsError::Io(format!("notion request failed: {error}")))?;
        let status = response.status();

        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|error| format!("<failed to read error body: {error}>"));
            return Err(AfsError::Io(format!(
                "notion api returned HTTP {status}: {body}"
            )));
        }

        response
            .json()
            .map_err(|error| AfsError::Io(format!("notion response decode failed: {error}")))
    }

    fn token(&self) -> AfsResult<String> {
        std::env::var(&self.config.token_key)
            .or_else(|_| std::env::var(DEFAULT_NOTION_TOKEN_ENV))
            .map_err(|_| {
                AfsError::InvalidState(format!(
                    "missing Notion token; set {}",
                    self.config.token_key
                ))
            })
    }
}

impl NotionApi for HttpNotionApi {
    fn retrieve_page(&self, page_id: &str) -> AfsResult<PageDto> {
        self.get_json(&format!("/v1/pages/{page_id}"), &[])
    }

    fn retrieve_block_children(
        &self,
        block_id: &str,
        start_cursor: Option<&str>,
    ) -> AfsResult<BlockListDto> {
        let mut query = vec![("page_size", "100".to_string())];
        if let Some(start_cursor) = start_cursor {
            query.push(("start_cursor", start_cursor.to_string()));
        }

        self.get_json(&format!("/v1/blocks/{block_id}/children"), &query)
    }
}
