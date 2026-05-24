use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

#[derive(Debug, Clone, Deserialize)]
pub struct DiscordMessage {
    pub id: String,
    pub channel_id: String,
    pub content: String,
    pub timestamp: String,
    pub author: Option<DiscordAuthor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscordAuthor {
    pub username: String,
    pub global_name: Option<String>,
}

impl DiscordAuthor {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscordThread {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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
        Self { http: Client::new(), token: token.into() }
    }

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let resp = self.http
            .get(format!("{DISCORD_API_BASE}{path}"))
            .header("Authorization", format!("Bot {}", self.token))
            .header("Accept", "application/json")
            .send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Discord API {status}: {text}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<DiscordChannel> {
        self.get(&format!("/channels/{channel_id}")).await
    }

    pub async fn get_messages(&self, channel_id: &str, limit: u8) -> Result<Vec<DiscordMessage>> {
        self.get(&format!("/channels/{channel_id}/messages?limit={limit}")).await
    }

    pub async fn get_message(&self, channel_id: &str, message_id: &str) -> Result<DiscordMessage> {
        self.get(&format!("/channels/{channel_id}/messages/{message_id}")).await
    }

    pub async fn get_guild_channels(&self, guild_id: &str) -> Result<Vec<DiscordChannel>> {
        self.get(&format!("/guilds/{guild_id}/channels")).await
    }

    pub async fn search_messages(&self, guild_id: &str, query: &str, channel_ids: &[String]) -> Result<Vec<DiscordMessage>> {
        let mut params = format!("content={}", url_encode(query));
        for id in channel_ids { params.push_str(&format!("&channel_id={id}")); }
        #[derive(Deserialize)]
        struct R { messages: Option<Vec<Vec<DiscordMessage>>> }
        let r: R = self.get(&format!("/guilds/{guild_id}/messages/search?{params}")).await?;
        Ok(r.messages.unwrap_or_default().into_iter().flatten().collect())
    }

    pub async fn get_forum_threads(&self, channel_id: &str, guild_id: &str) -> Result<Vec<DiscordThread>> {
        #[derive(Deserialize)] struct R { threads: Vec<DiscordThread> }
        let active: R = self.get(&format!("/guilds/{guild_id}/threads/active")).await?;
        let archived: R = self.get(&format!("/channels/{channel_id}/threads/archived/public?limit=100")).await?;
        let active_here: Vec<DiscordThread> = active.threads.into_iter()
            .filter(|t| t.parent_id.as_deref() == Some(channel_id)).collect();
        Ok(active_here.into_iter().chain(archived.threads).collect())
    }

    pub async fn fetch_thread_messages(&self, channel_id: &str, lookback: usize) -> Result<Vec<(DiscordThread, Vec<DiscordMessage>)>> {
        #[derive(Deserialize)] struct R { threads: Vec<DiscordThread> }
        let r: R = self.get(&format!("/channels/{channel_id}/threads/archived/public?limit={lookback}")).await?;
        let mut out = Vec::new();
        for thread in r.threads {
            let messages = self.get_all_messages(&thread.id).await?;
            out.push((thread, messages));
        }
        Ok(out)
    }

    async fn get_all_messages(&self, channel_id: &str) -> Result<Vec<DiscordMessage>> {
        let newest: Vec<DiscordMessage> = self.get(&format!("/channels/{channel_id}/messages?limit=1")).await?;
        if newest.is_empty() { return Ok(vec![]); }
        let mut all = newest;
        loop {
            let before = all.last().unwrap().id.clone();
            let batch: Vec<DiscordMessage> = self.get(&format!("/channels/{channel_id}/messages?limit=100&before={before}")).await?;
            if batch.is_empty() { break; }
            all.extend(batch);
        }
        all.reverse();
        Ok(all)
    }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}
