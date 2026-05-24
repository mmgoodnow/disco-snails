use anyhow::Result;
use chrono::DateTime;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{get_thread_summary, upsert_thread_summary};
use crate::discord::DiscordClient;
use crate::summarizer::{TranscriptMessage, summarize_thread};

pub fn discord_ts_to_ms(ts: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(ts).ok().map(|dt| dt.timestamp_millis())
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

pub async fn process_threads(
    pool: &SqlitePool,
    discord: &DiscordClient,
    forum_channel_id: &str,
    lookback: usize,
    model: &str,
    verbose: bool,
) -> Result<()> {
    let thread_batches = discord.fetch_thread_messages(forum_channel_id, lookback).await?;
    let mut processed = 0usize;
    let mut skipped = 0usize;

    for (thread, messages) in thread_batches {
        if messages.is_empty() {
            if verbose { println!("Thread {:?}: no messages, skipping", thread.name); }
            skipped += 1;
            continue;
        }

        let last_ts = messages.iter().filter_map(|m| discord_ts_to_ms(&m.timestamp)).max().unwrap_or(0);

        if let Some(existing) = get_thread_summary(pool, &thread.id).await? {
            if existing.last_message_timestamp == last_ts {
                if verbose { println!("Thread {:?}: up to date, skipping", thread.name); }
                skipped += 1;
                continue;
            }
        }

        let transcript: Vec<TranscriptMessage> = messages.iter().map(|m| TranscriptMessage {
            user: m.author.as_ref().map(|a| a.display_name().to_string()).unwrap_or_else(|| "Unknown".to_string()),
            content: m.content.clone(),
        }).collect();

        println!("Summarizing {:?}", thread.name);
        let ai_summary = summarize_thread(&thread.name, &transcript, model).await?;
        let transcript_json = serde_json::to_string(&transcript)?;

        upsert_thread_summary(pool, &thread.id, &thread.name, &transcript_json, &ai_summary, last_ts, now_ms()).await?;
        processed += 1;
    }

    if processed == 0 {
        println!("{skipped} threads were up-to-date; no new summaries.");
    } else {
        println!("Processed {processed} threads ({skipped} skipped without changes).");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discord_ts_to_ms_parses_rfc3339() {
        // 2023-11-14T22:13:20+00:00 = 1700000000000 ms
        assert_eq!(discord_ts_to_ms("2023-11-14T22:13:20+00:00"), Some(1_700_000_000_000));
    }

    #[test]
    fn discord_ts_to_ms_with_microseconds() {
        assert_eq!(discord_ts_to_ms("2023-11-14T22:13:20.000000+00:00"), Some(1_700_000_000_000));
    }

    #[test]
    fn discord_ts_to_ms_invalid_returns_none() {
        assert_eq!(discord_ts_to_ms("not-a-date"), None);
    }
}
