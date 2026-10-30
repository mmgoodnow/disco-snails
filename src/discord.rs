use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const MAX_RATE_LIMIT_RETRIES: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordMessage {
    pub id: String,
    pub channel_id: String,
    pub content: String,
    pub timestamp: String,
    pub author: Option<DiscordAuthor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordAuthor {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl DiscordAuthor {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub bot: bool,
}

impl DiscordUser {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGuild {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub owner: bool,
    #[serde(default)]
    pub permissions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordThread {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordChannel {
    pub id: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: u8,
    pub guild_id: Option<String>,
    pub parent_id: Option<String>,
    pub topic: Option<String>,
}

pub struct DiscordClient {
    http: Client,
    token: String,
}

impl DiscordClient {
    pub fn new(token: impl Into<String>) -> Self {
        let http = Client::builder()
            .user_agent(concat!("disco/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("valid reqwest client configuration");
        Self {
            http,
            token: token.into(),
        }
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<T> {
        let value = self.request_value(method, path, body).await?;
        serde_json::from_value(value)
            .with_context(|| format!("Discord returned an unexpected response for {path}"))
    }

    async fn request_value(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        if !path.starts_with('/') || path.contains("://") {
            bail!("Discord API path must be relative and start with '/': {path:?}");
        }

        for attempt in 0..=MAX_RATE_LIMIT_RETRIES {
            let mut request = self
                .http
                .request(method.clone(), format!("{DISCORD_API_BASE}{path}"))
                .header("Authorization", format!("Bot {}", self.token))
                .header("Accept", "application/json");
            if let Some(body) = body {
                request = request.json(body);
            }

            let response = request.send().await?;
            let status = response.status();
            let bytes = response.bytes().await?;

            if status == StatusCode::TOO_MANY_REQUESTS {
                let rate_limit: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let retry_after = rate_limit
                    .get("retry_after")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0)
                    .max(0.05);
                if attempt == MAX_RATE_LIMIT_RETRIES {
                    return Err(anyhow!(
                        "Discord API rate limit persisted after {} retries",
                        MAX_RATE_LIMIT_RETRIES
                    ));
                }
                tokio::time::sleep(Duration::from_secs_f64(retry_after)).await;
                continue;
            }

            if !status.is_success() {
                let text = String::from_utf8_lossy(&bytes);
                let excerpt: String = text.chars().take(2_000).collect();
                return Err(anyhow!("Discord API {status}: {excerpt}"));
            }

            if bytes.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_slice(&bytes)
                .with_context(|| format!("Discord returned invalid JSON for {path}"));
        }

        unreachable!("rate limit loop always returns")
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::GET, path, None).await
    }

    pub async fn get_raw(&self, path: &str) -> Result<Value> {
        self.request_value(Method::GET, path, None).await
    }

    pub async fn get_current_user(&self) -> Result<DiscordUser> {
        self.get("/users/@me").await
    }

    pub async fn get_current_user_guilds(&self) -> Result<Vec<DiscordGuild>> {
        self.get("/users/@me/guilds?limit=200").await
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<DiscordChannel> {
        self.get(&format!("/channels/{channel_id}")).await
    }

    pub async fn get_messages(&self, channel_id: &str, limit: u8) -> Result<Vec<DiscordMessage>> {
        self.get_messages_page(channel_id, limit, None, None, None)
            .await
    }

    pub async fn get_messages_page(
        &self,
        channel_id: &str,
        limit: u8,
        before: Option<&str>,
        after: Option<&str>,
        around: Option<&str>,
    ) -> Result<Vec<DiscordMessage>> {
        if !(1..=100).contains(&limit) {
            bail!("message limit must be between 1 and 100");
        }
        if [before, after, around].iter().flatten().count() > 1 {
            bail!("only one of before, after, or around may be specified");
        }

        let mut query = format!("limit={limit}");
        if let Some(value) = before {
            query.push_str(&format!("&before={value}"));
        }
        if let Some(value) = after {
            query.push_str(&format!("&after={value}"));
        }
        if let Some(value) = around {
            query.push_str(&format!("&around={value}"));
        }
        self.get(&format!("/channels/{channel_id}/messages?{query}"))
            .await
    }

    pub async fn get_message(&self, channel_id: &str, message_id: &str) -> Result<DiscordMessage> {
        self.get(&format!("/channels/{channel_id}/messages/{message_id}"))
            .await
    }

    pub async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
        allow_mentions: bool,
    ) -> Result<DiscordMessage> {
        if content.trim().is_empty() {
            bail!("message content cannot be empty");
        }
        let body = if allow_mentions {
            json!({ "content": content })
        } else {
            json!({ "content": content, "allowed_mentions": { "parse": [] } })
        };
        self.request(
            Method::POST,
            &format!("/channels/{channel_id}/messages"),
            Some(&body),
        )
        .await
    }

    pub async fn get_guild_channels(&self, guild_id: &str) -> Result<Vec<DiscordChannel>> {
        self.get(&format!("/guilds/{guild_id}/channels")).await
    }

    pub async fn search_messages(
        &self,
        guild_id: &str,
        query: &str,
        channel_ids: &[String],
    ) -> Result<Vec<DiscordMessage>> {
        let mut params = format!("content={}", url_encode(query));
        for id in channel_ids {
            params.push_str(&format!("&channel_id={id}"));
        }
        #[derive(Deserialize)]
        struct R {
            messages: Option<Vec<Vec<DiscordMessage>>>,
        }
        let r: R = self
            .get(&format!("/guilds/{guild_id}/messages/search?{params}"))
            .await?;
        Ok(r.messages
            .unwrap_or_default()
            .into_iter()
            .flatten()
            .collect())
    }

    pub async fn get_forum_threads(
        &self,
        channel_id: &str,
        guild_id: &str,
    ) -> Result<Vec<DiscordThread>> {
        #[derive(Deserialize)]
        struct R {
            threads: Vec<DiscordThread>,
        }
        let active: R = self
            .get(&format!("/guilds/{guild_id}/threads/active"))
            .await?;
        let archived: R = self
            .get(&format!(
                "/channels/{channel_id}/threads/archived/public?limit=100"
            ))
            .await?;
        let mut threads: Vec<DiscordThread> = active
            .threads
            .into_iter()
            .filter(|t| t.parent_id.as_deref() == Some(channel_id))
            .collect();
        for thread in archived.threads {
            if !threads.iter().any(|existing| existing.id == thread.id) {
                threads.push(thread);
            }
        }
        Ok(threads)
    }

    pub async fn fetch_thread_messages(
        &self,
        channel_id: &str,
        lookback: usize,
    ) -> Result<Vec<(DiscordThread, Vec<DiscordMessage>)>> {
        #[derive(Deserialize)]
        struct R {
            threads: Vec<DiscordThread>,
        }
        let lookback = lookback.clamp(1, 100);
        let r: R = self
            .get(&format!(
                "/channels/{channel_id}/threads/archived/public?limit={lookback}"
            ))
            .await?;
        let mut out = Vec::new();
        for thread in r.threads {
            let messages = self.get_all_messages(&thread.id).await?;
            out.push((thread, messages));
        }
        Ok(out)
    }

    pub async fn get_all_messages(&self, channel_id: &str) -> Result<Vec<DiscordMessage>> {
        let newest: Vec<DiscordMessage> = self
            .get(&format!("/channels/{channel_id}/messages?limit=1"))
            .await?;
        if newest.is_empty() {
            return Ok(vec![]);
        }
        let mut all = newest;
        loop {
            let before = all.last().unwrap().id.clone();
            let batch: Vec<DiscordMessage> = self
                .get(&format!(
                    "/channels/{channel_id}/messages?limit=100&before={before}"
                ))
                .await?;
            if batch.is_empty() {
                break;
            }
            all.extend(batch);
        }
        all.reverse();
        Ok(all)
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::url_encode;

    #[test]
    fn encodes_search_terms() {
        assert_eq!(url_encode("hello world/ok"), "hello%20world%2Fok");
    }
}
