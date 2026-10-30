use std::{
    env,
    io::{self, IsTerminal, Read},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Serialize;
use serde_json::json;

use crate::{
    discord::{DiscordChannel, DiscordClient, DiscordMessage},
    service::{self, ServiceConfig},
    summarizer::{self, SummaryFormat, TranscriptMessage},
    target::{message_target_from_ids, parse_channel_id, parse_message_target},
};

#[derive(Debug, Parser)]
#[command(
    name = "disco",
    version,
    about = "A practical Discord CLI and optional local service"
)]
struct Cli {
    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect the configured Discord bot identity.
    Auth(AuthArgs),
    /// Work with Discord guilds (servers).
    Guild(GuildArgs),
    /// Work with channels.
    Channel(ChannelArgs),
    /// Read, search, and send messages.
    Message(MessageArgs),
    /// List, read, and summarize threads.
    Thread(ThreadArgs),
    /// Make a read-only request to a Discord API path.
    Api(ApiArgs),
    /// Run the HTTP/MCP server and scheduled summarizer.
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct AuthArgs {
    #[command(subcommand)]
    command: AuthCommand,
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    /// Validate the bot token and print its identity.
    Status,
}

#[derive(Debug, Args)]
struct GuildArgs {
    #[command(subcommand)]
    command: GuildCommand,
}

#[derive(Debug, Subcommand)]
enum GuildCommand {
    /// List guilds visible to the bot.
    List,
}

#[derive(Debug, Args)]
struct ChannelArgs {
    #[command(subcommand)]
    command: ChannelCommand,
}

#[derive(Debug, Subcommand)]
enum ChannelCommand {
    /// List channels in a guild.
    List(ChannelListArgs),
    /// Get one channel by ID or Discord URL.
    Get(ChannelGetArgs),
}

#[derive(Debug, Args)]
struct ChannelListArgs {
    /// Guild ID. Defaults to DISCORD_GUILD_ID, or the configured forum's guild.
    #[arg(long)]
    guild: Option<String>,
}

#[derive(Debug, Args)]
struct ChannelGetArgs {
    /// Channel/thread ID or Discord URL.
    channel: String,
}

#[derive(Debug, Args)]
struct MessageArgs {
    #[command(subcommand)]
    command: MessageCommand,
}

#[derive(Debug, Subcommand)]
enum MessageCommand {
    /// List recent messages in a channel or thread.
    List(MessageListArgs),
    /// Get a message from a Discord URL, or from channel and message IDs.
    Get(MessageGetArgs),
    /// Search messages in a guild.
    Search(MessageSearchArgs),
    /// Send a message. Reads stdin when CONTENT is omitted.
    Send(MessageSendArgs),
}

#[derive(Debug, Args)]
struct MessageListArgs {
    /// Channel/thread ID or Discord URL.
    channel: String,
    /// Number of messages to return (1-100).
    #[arg(short = 'n', long, default_value_t = 25)]
    limit: u8,
    /// Return messages before this message ID.
    #[arg(long, conflicts_with_all = ["after", "around"])]
    before: Option<String>,
    /// Return messages after this message ID.
    #[arg(long, conflicts_with_all = ["before", "around"])]
    after: Option<String>,
    /// Return messages around this message ID.
    #[arg(long, conflicts_with_all = ["before", "after"])]
    around: Option<String>,
}

#[derive(Debug, Args)]
struct MessageGetArgs {
    /// Discord message URL/URI, or a channel ID when MESSAGE_ID is supplied.
    target: String,
    /// Message ID when TARGET is a channel ID.
    message_id: Option<String>,
}

#[derive(Debug, Args)]
struct MessageSearchArgs {
    /// Search text.
    query: String,
    /// Guild ID. Defaults to DISCORD_GUILD_ID, or the configured forum's guild.
    #[arg(long)]
    guild: Option<String>,
    /// Restrict search to one or more channel IDs/URLs.
    #[arg(long = "channel")]
    channels: Vec<String>,
    /// Maximum results printed.
    #[arg(short = 'n', long, default_value_t = 25)]
    limit: usize,
}

#[derive(Debug, Args)]
struct MessageSendArgs {
    /// Channel/thread ID or Discord URL.
    channel: String,
    /// Message content. If omitted, content is read from stdin.
    content: Option<String>,
    /// Allow Discord to parse user, role, and @everyone mentions.
    #[arg(long)]
    allow_mentions: bool,
}

#[derive(Debug, Args)]
struct ThreadArgs {
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    /// List active and archived threads in a forum.
    List(ThreadListArgs),
    /// Read messages from a thread.
    Read(ThreadReadArgs),
    /// Summarize a thread with OpenAI.
    Summarize(ThreadSummarizeArgs),
}

#[derive(Debug, Args)]
struct ThreadListArgs {
    /// Forum channel ID or Discord URL. Defaults to DISCORD_FORUM_CHANNEL_ID.
    forum: Option<String>,
    /// Guild ID. Usually inferred from the forum.
    #[arg(long)]
    guild: Option<String>,
}

#[derive(Debug, Args)]
struct ThreadReadArgs {
    /// Thread ID or Discord URL.
    thread: String,
    /// Number of recent messages to return when --all is not set (1-100).
    #[arg(short = 'n', long, default_value_t = 100)]
    limit: u8,
    /// Fetch the complete thread history.
    #[arg(long)]
    all: bool,
}

#[derive(Debug, Args)]
struct ThreadSummarizeArgs {
    /// Thread ID or Discord URL.
    thread: String,
    /// Number of recent messages to summarize when --all is not set (1-100).
    #[arg(short = 'n', long, default_value_t = 100)]
    limit: u8,
    /// Summarize the complete thread history.
    #[arg(long)]
    all: bool,
    /// OpenAI model. Defaults to OPENAI_MODEL or gpt-5-mini.
    #[arg(long)]
    model: Option<String>,
    /// Extra domain or project context for the summary.
    #[arg(long)]
    context: Option<String>,
}

#[derive(Debug, Args)]
struct ApiArgs {
    #[command(subcommand)]
    command: ApiCommand,
}

#[derive(Debug, Subcommand)]
enum ApiCommand {
    /// GET a documented API path, such as /users/@me.
    Get(ApiGetArgs),
}

#[derive(Debug, Args)]
struct ApiGetArgs {
    /// Discord API path, with or without a leading slash.
    path: String,
}

#[derive(Debug, Default, Args)]
struct ServeArgs {
    /// Forum channel to process. Defaults to DISCORD_FORUM_CHANNEL_ID.
    #[arg(long)]
    forum: Option<String>,
    /// Guild containing the forum. Defaults to DISCORD_GUILD_ID.
    #[arg(long)]
    guild: Option<String>,
    /// HTTP bind address. Defaults to BIND_ADDRESS or 0.0.0.0.
    #[arg(long)]
    bind: Option<String>,
    /// HTTP port. Defaults to PORT or 80.
    #[arg(long)]
    port: Option<u16>,
    /// SQLite URL. Defaults to DB_PATH or sqlite:snails.db.
    #[arg(long)]
    db: Option<String>,
    /// Number of archived threads processed per cycle.
    #[arg(long)]
    lookback: Option<usize>,
    /// OpenAI model used for summaries.
    #[arg(long)]
    model: Option<String>,
    /// Seconds between processing cycles. Defaults to POLL_INTERVAL_SECONDS or 86400.
    #[arg(long)]
    interval: Option<u64>,
    /// Log skipped/up-to-date threads.
    #[arg(short, long)]
    verbose: bool,
}

pub async fn run(default_to_serve: bool) -> Result<()> {
    load_environment();
    let _ = tracing_subscriber::fmt::try_init();
    let cli = Cli::parse();

    let Some(command) = cli.command else {
        if default_to_serve {
            return run_service(ServeArgs::default()).await;
        }
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    match command {
        Command::Auth(args) => run_auth(args, cli.json).await,
        Command::Guild(args) => run_guild(args, cli.json).await,
        Command::Channel(args) => run_channel(args, cli.json).await,
        Command::Message(args) => run_message(args, cli.json).await,
        Command::Thread(args) => run_thread(args, cli.json).await,
        Command::Api(args) => run_api(args).await,
        Command::Serve(args) => run_service(args).await,
    }
}

fn load_environment() {
    let _ = dotenvy::from_path("/config/.env");
    let _ = dotenvy::dotenv();
}

fn env_value(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn required_env(key: &str) -> Result<String> {
    env_value(key).with_context(|| format!("{key} is not set"))
}

fn discord_client() -> Result<DiscordClient> {
    Ok(DiscordClient::new(required_env("DISCORD_BOT_TOKEN")?))
}

async fn run_auth(args: AuthArgs, json_output: bool) -> Result<()> {
    match args.command {
        AuthCommand::Status => {
            let user = discord_client()?.get_current_user().await?;
            if json_output {
                print_json(&user)?;
            } else {
                println!(
                    "Authenticated as {} ({}){}",
                    user.display_name(),
                    user.id,
                    if user.bot { " [bot]" } else { "" }
                );
            }
        }
    }
    Ok(())
}

async fn run_guild(args: GuildArgs, json_output: bool) -> Result<()> {
    match args.command {
        GuildCommand::List => {
            let mut guilds = discord_client()?.get_current_user_guilds().await?;
            guilds.sort_by_key(|guild| guild.name.to_lowercase());
            if json_output {
                print_json(&guilds)?;
            } else if guilds.is_empty() {
                println!("No guilds visible to this bot.");
            } else {
                for guild in guilds {
                    println!("{}\t{}", guild.id, guild.name);
                }
            }
        }
    }
    Ok(())
}

async fn run_channel(args: ChannelArgs, json_output: bool) -> Result<()> {
    let client = discord_client()?;
    match args.command {
        ChannelCommand::List(args) => {
            let guild_id = resolve_guild_id(&client, args.guild).await?;
            let mut channels = client.get_guild_channels(&guild_id).await?;
            channels.sort_by(|left, right| {
                left.name
                    .as_deref()
                    .unwrap_or(&left.id)
                    .to_lowercase()
                    .cmp(&right.name.as_deref().unwrap_or(&right.id).to_lowercase())
            });
            if json_output {
                print_json(&channels)?;
            } else if channels.is_empty() {
                println!("No channels visible to this bot.");
            } else {
                for channel in channels {
                    print_channel(&channel);
                }
            }
        }
        ChannelCommand::Get(args) => {
            let channel = client
                .get_channel(&parse_channel_id(&args.channel)?)
                .await?;
            if json_output {
                print_json(&channel)?;
            } else {
                print_channel(&channel);
            }
        }
    }
    Ok(())
}

async fn run_message(args: MessageArgs, json_output: bool) -> Result<()> {
    let client = discord_client()?;
    match args.command {
        MessageCommand::List(args) => {
            let channel_id = parse_channel_id(&args.channel)?;
            let mut messages = client
                .get_messages_page(
                    &channel_id,
                    args.limit,
                    args.before.as_deref(),
                    args.after.as_deref(),
                    args.around.as_deref(),
                )
                .await?;
            messages.reverse();
            output_messages(&messages, json_output)?;
        }
        MessageCommand::Get(args) => {
            let target = match args.message_id {
                Some(message_id) => message_target_from_ids(&args.target, &message_id)?,
                None => parse_message_target(&args.target)?,
            };
            let message = client
                .get_message(&target.channel_id, &target.message_id)
                .await?;
            if json_output {
                print_json(&message)?;
            } else {
                print_message(&message);
            }
        }
        MessageCommand::Search(args) => {
            if args.query.trim().is_empty() {
                bail!("search query cannot be empty");
            }
            let guild_id = resolve_guild_id(&client, args.guild).await?;
            let channel_ids = args
                .channels
                .iter()
                .map(|channel| parse_channel_id(channel))
                .collect::<Result<Vec<_>>>()?;
            let mut messages = client
                .search_messages(&guild_id, &args.query, &channel_ids)
                .await?;
            messages.truncate(args.limit);
            output_messages(&messages, json_output)?;
        }
        MessageCommand::Send(args) => {
            let channel_id = parse_channel_id(&args.channel)?;
            let content = read_message_content(args.content)?;
            let message = client
                .send_message(&channel_id, content.trim_end(), args.allow_mentions)
                .await?;
            if json_output {
                print_json(&message)?;
            } else {
                println!(
                    "Sent message {} to channel {}.",
                    message.id, message.channel_id
                );
            }
        }
    }
    Ok(())
}

async fn run_thread(args: ThreadArgs, json_output: bool) -> Result<()> {
    let client = discord_client()?;
    match args.command {
        ThreadCommand::List(args) => {
            let forum = args
                .forum
                .or_else(|| env_value("DISCORD_FORUM_CHANNEL_ID"))
                .context("provide a forum channel or set DISCORD_FORUM_CHANNEL_ID")?;
            let forum_id = parse_channel_id(&forum)?;
            let guild_id = match args.guild.or_else(|| env_value("DISCORD_GUILD_ID")) {
                Some(guild) => guild,
                None => client
                    .get_channel(&forum_id)
                    .await?
                    .guild_id
                    .context("forum response did not include a guild ID")?,
            };
            let mut threads = client.get_forum_threads(&forum_id, &guild_id).await?;
            threads.sort_by_key(|thread| thread.name.to_lowercase());
            if json_output {
                print_json(&threads)?;
            } else if threads.is_empty() {
                println!("No active or public archived threads found.");
            } else {
                for thread in threads {
                    println!("{}\t{}", thread.id, thread.name);
                }
            }
        }
        ThreadCommand::Read(args) => {
            let thread_id = parse_channel_id(&args.thread)?;
            let messages = read_thread_messages(&client, &thread_id, args.limit, args.all).await?;
            output_messages(&messages, json_output)?;
        }
        ThreadCommand::Summarize(args) => {
            let thread_id = parse_channel_id(&args.thread)?;
            let channel = client.get_channel(&thread_id).await?;
            let messages = read_thread_messages(&client, &thread_id, args.limit, args.all).await?;
            if messages.is_empty() {
                bail!("thread contains no readable messages");
            }
            let transcript: Vec<TranscriptMessage> = messages
                .iter()
                .map(|message| TranscriptMessage {
                    user: message
                        .author
                        .as_ref()
                        .map(|author| author.display_name().to_string())
                        .unwrap_or_else(|| "Unknown".to_string()),
                    content: message.content.clone(),
                })
                .collect();
            let title = channel.name.unwrap_or_else(|| thread_id.clone());
            let model = args
                .model
                .or_else(|| env_value("OPENAI_MODEL"))
                .unwrap_or_else(|| "gpt-5-mini".to_string());
            let summary = summarizer::summarize_thread_with_context(
                &title,
                &transcript,
                &model,
                args.context.as_deref(),
                SummaryFormat::Markdown,
            )
            .await?;
            if json_output {
                print_json(&json!({
                    "thread_id": thread_id,
                    "title": title,
                    "model": model,
                    "message_count": transcript.len(),
                    "summary": summary,
                }))?;
            } else {
                println!("# {title}\n\n{summary}");
            }
        }
    }
    Ok(())
}

async fn run_api(args: ApiArgs) -> Result<()> {
    match args.command {
        ApiCommand::Get(args) => {
            let path = if args.path.starts_with('/') {
                args.path
            } else {
                format!("/{}", args.path)
            };
            let value = discord_client()?.get_raw(&path).await?;
            print_json(&value)?;
        }
    }
    Ok(())
}

async fn run_service(args: ServeArgs) -> Result<()> {
    let config = ServiceConfig {
        bot_token: required_env("DISCORD_BOT_TOKEN")?,
        forum_channel_id: args
            .forum
            .or_else(|| env_value("DISCORD_FORUM_CHANNEL_ID"))
            .context("DISCORD_FORUM_CHANNEL_ID is not set; provide --forum")?,
        guild_id: args.guild.or_else(|| env_value("DISCORD_GUILD_ID")),
        web_api_key: env_value("WEB_API_KEY"),
        openai_model: args
            .model
            .or_else(|| env_value("OPENAI_MODEL"))
            .unwrap_or_else(|| "gpt-5-mini".to_string()),
        lookback: args.lookback.or_else(|| parse_env("LOOKBACK")).unwrap_or(2),
        verbose: args.verbose || env_value("DISCORD_VERBOSE_LOGS").as_deref() == Some("true"),
        bind_address: args
            .bind
            .or_else(|| env_value("BIND_ADDRESS"))
            .unwrap_or_else(|| "0.0.0.0".to_string()),
        port: args.port.or_else(|| parse_env("PORT")).unwrap_or(80),
        db_path: args
            .db
            .or_else(|| env_value("DB_PATH"))
            .unwrap_or_else(|| "sqlite:snails.db".to_string()),
        interval: Duration::from_secs(
            args.interval
                .or_else(|| parse_env("POLL_INTERVAL_SECONDS"))
                .unwrap_or(24 * 60 * 60),
        ),
    };
    service::run(config).await
}

async fn resolve_guild_id(client: &DiscordClient, explicit: Option<String>) -> Result<String> {
    if let Some(guild_id) = explicit.or_else(|| env_value("DISCORD_GUILD_ID")) {
        return Ok(guild_id);
    }
    if let Some(forum) = env_value("DISCORD_FORUM_CHANNEL_ID") {
        return client
            .get_channel(&parse_channel_id(&forum)?)
            .await?
            .guild_id
            .context("configured forum response did not include a guild ID");
    }
    bail!("provide --guild, set DISCORD_GUILD_ID, or configure DISCORD_FORUM_CHANNEL_ID")
}

async fn read_thread_messages(
    client: &DiscordClient,
    thread_id: &str,
    limit: u8,
    all: bool,
) -> Result<Vec<DiscordMessage>> {
    if all {
        client.get_all_messages(thread_id).await
    } else {
        let mut messages = client.get_messages(thread_id, limit).await?;
        messages.reverse();
        Ok(messages)
    }
}

fn read_message_content(argument: Option<String>) -> Result<String> {
    if let Some(content) = argument {
        return Ok(content);
    }
    if io::stdin().is_terminal() {
        bail!("provide message content as an argument or pipe it on stdin");
    }
    let mut content = String::new();
    io::stdin().read_to_string(&mut content)?;
    if content.trim().is_empty() {
        bail!("message content cannot be empty");
    }
    Ok(content)
}

fn output_messages(messages: &[DiscordMessage], json_output: bool) -> Result<()> {
    if json_output {
        print_json(messages)?;
    } else if messages.is_empty() {
        println!("No messages found.");
    } else {
        for message in messages {
            print_message(message);
        }
    }
    Ok(())
}

fn print_message(message: &DiscordMessage) {
    let author = message
        .author
        .as_ref()
        .map(|author| author.display_name())
        .unwrap_or("Unknown");
    println!("{}  {}  [{}]", message.timestamp, author, message.id);
    println!(
        "{}\n",
        if message.content.is_empty() {
            "[no textual content]"
        } else {
            &message.content
        }
    );
}

fn print_channel(channel: &DiscordChannel) {
    println!(
        "{}\t{}\t{}",
        channel.id,
        channel_type_name(channel.kind),
        channel.name.as_deref().unwrap_or("[unnamed]")
    );
}

fn channel_type_name(kind: u8) -> &'static str {
    match kind {
        0 => "text",
        1 => "dm",
        2 => "voice",
        3 => "group-dm",
        4 => "category",
        5 => "announcement",
        10 => "announcement-thread",
        11 => "public-thread",
        12 => "private-thread",
        13 => "stage",
        14 => "directory",
        15 => "forum",
        16 => "media",
        _ => "unknown",
    }
}

fn print_json<T: Serialize + ?Sized>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn parse_env<T>(key: &str) -> Option<T>
where
    T: std::str::FromStr,
{
    env_value(key)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_message_list_command() {
        let cli =
            Cli::try_parse_from(["disco", "message", "list", "123", "--limit", "10", "--json"])
                .unwrap();
        assert!(cli.json);
        let Some(Command::Message(MessageArgs {
            command: MessageCommand::List(args),
        })) = cli.command
        else {
            panic!("wrong command parsed");
        };
        assert_eq!(args.channel, "123");
        assert_eq!(args.limit, 10);
    }

    #[test]
    fn parses_serve_options() {
        let cli = Cli::try_parse_from([
            "disco",
            "serve",
            "--forum",
            "123",
            "--port",
            "8080",
            "--interval",
            "60",
        ])
        .unwrap();
        let Some(Command::Serve(args)) = cli.command else {
            panic!("wrong command parsed");
        };
        assert_eq!(args.forum.as_deref(), Some("123"));
        assert_eq!(args.port, Some(8080));
        assert_eq!(args.interval, Some(60));
    }
}
