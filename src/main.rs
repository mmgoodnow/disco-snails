mod db;
mod discord;
mod processor;
mod server;
mod summarizer;

use std::sync::Arc;
use std::time::Duration;
use anyhow::{Context, Result};
use tokio::net::TcpListener;

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("{key} is not set"))
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env — try cwd first, then /config/.env (production mount path)
    if dotenvy::dotenv().is_err() {
        let _ = dotenvy::from_path("/config/.env");
    }
    tracing_subscriber::fmt::init();

    let bot_token = env("DISCORD_BOT_TOKEN")?;
    let forum_channel_id = env("DISCORD_FORUM_CHANNEL_ID")?;
    let guild_id = env_opt("DISCORD_GUILD_ID");
    let web_api_key = env_opt("WEB_API_KEY");
    let openai_model = env_opt("OPENAI_MODEL").unwrap_or_else(|| "gpt-5-mini".to_string());
    let lookback: usize = env_opt("LOOKBACK").and_then(|v| v.parse().ok()).unwrap_or(2);
    let verbose = env_opt("DISCORD_VERBOSE_LOGS").map(|v| v == "true").unwrap_or(false);
    let port: u16 = env_opt("PORT").and_then(|v| v.parse().ok()).unwrap_or(80);
    let db_path = env_opt("DB_PATH").unwrap_or_else(|| "sqlite:snails.db".to_string());

    let pool = db::open_db(&db_path).await?;
    let discord = Arc::new(discord::DiscordClient::new(&bot_token));

    let state = server::AppState {
        pool: Arc::new(pool.clone()),
        discord: Arc::clone(&discord),
        web_api_key,
        forum_channel_id: Some(forum_channel_id.clone()),
        guild_id: guild_id.clone(),
    };
    let app = server::router(state);
    let listener = TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    println!("Server listening on http://0.0.0.0:{port}");
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });

    loop {
        if let Err(e) = processor::process_threads(&pool, &discord, &forum_channel_id, lookback, &openai_model, verbose).await {
            eprintln!("Failed to process Discord threads: {e}");
        }
        tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
    }
}
