use std::{sync::Arc, time::Duration};

use anyhow::Result;
use tokio::net::TcpListener;

use crate::{
    db,
    discord::DiscordClient,
    processor,
    server::{self, AppState},
};

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub bot_token: String,
    pub forum_channel_id: String,
    pub guild_id: Option<String>,
    pub web_api_key: Option<String>,
    pub openai_model: String,
    pub lookback: usize,
    pub verbose: bool,
    pub bind_address: String,
    pub port: u16,
    pub db_path: String,
    pub interval: Duration,
}

pub async fn run(config: ServiceConfig) -> Result<()> {
    let pool = db::open_db(&config.db_path).await?;
    let discord = Arc::new(DiscordClient::new(&config.bot_token));

    let state = AppState {
        pool: Arc::new(pool.clone()),
        discord: Arc::clone(&discord),
        web_api_key: config.web_api_key,
        forum_channel_id: Some(config.forum_channel_id.clone()),
        guild_id: config.guild_id,
    };
    let app = server::router(state);
    let address = format!("{}:{}", config.bind_address, config.port);
    let listener = TcpListener::bind(&address).await?;
    println!("Server listening on http://{address}");
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app).await {
            tracing::error!(%error, "HTTP server stopped");
        }
    });

    loop {
        if let Err(error) = processor::process_threads(
            &pool,
            &discord,
            &config.forum_channel_id,
            config.lookback,
            &config.openai_model,
            config.verbose,
        )
        .await
        {
            tracing::error!(%error, "failed to process Discord threads");
        }
        tokio::time::sleep(config.interval).await;
    }
}
