# disco

`disco` is a local-first Discord CLI with an optional long-running service. It uses a Discord bot account; it does not automate a normal user account.

## Build

```bash
cargo build --release --bin disco
./target/release/disco --help
```

The legacy `disco-snails` binary remains available and starts service mode when invoked without a subcommand. The primary `disco` binary shows help when invoked without a subcommand.

## Configuration

`disco` loads `/config/.env` when present, then searches for a `.env` file from the current directory upward.

```dotenv
DISCORD_BOT_TOKEN=your-bot-token

# Optional defaults
DISCORD_GUILD_ID=123456789012345678
DISCORD_FORUM_CHANNEL_ID=234567890123456789

# Required only for AI summaries
OPENAI_API_KEY=your-openai-key
OPENAI_MODEL=gpt-5-mini
```

Keep the configuration file private (`chmod 600 .env`). The CLI never prints the bot token.

## Commands

```bash
# Verify authentication
disco auth status

# Discover servers and channels
disco guild list
disco channel list --guild 123456789012345678
disco channel get 234567890123456789

# Read messages. IDs, discord:// URIs, and discord.com URLs are accepted.
disco message list 234567890123456789 --limit 25
disco message get https://discord.com/channels/GUILD/CHANNEL/MESSAGE
disco message search "release notes" --guild 123456789012345678

# Send safely. Mentions are disabled unless --allow-mentions is supplied.
disco message send 234567890123456789 "hello from disco"
printf 'multiline message\n' | disco message send 234567890123456789

# Work with forum threads
disco thread list 234567890123456789
disco thread read THREAD_ID --all
disco thread summarize THREAD_ID --context "This is a project support forum"

# Read-only escape hatch for documented Discord endpoints
disco api get /users/@me

# Add --json to any resource command for machine-readable output
disco guild list --json
```

Run `disco <resource> <command> --help` for all options.

## Service mode

Service mode preserves the original application: it serves the HTML page, JSON Feed, and MCP endpoint while periodically summarizing recent archived forum threads.

```bash
disco serve --forum 234567890123456789 --port 8080
```

The service also supports these environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `DISCORD_FORUM_CHANNEL_ID` | required | Forum processed by the worker |
| `DISCORD_GUILD_ID` | inferred where possible | Guild used by live search/MCP |
| `DB_PATH` | `sqlite:snails.db` | SQLite database URL |
| `PORT` | `80` | HTTP port |
| `BIND_ADDRESS` | `0.0.0.0` | HTTP bind address |
| `LOOKBACK` | `2` | Archived threads processed per cycle |
| `POLL_INTERVAL_SECONDS` | `86400` | Worker interval |
| `WEB_API_KEY` | unset | Bearer key protecting HTTP endpoints |

The JSON Feed is available at `/feed.json`, and MCP requests are accepted at `/mcp`.

## Docker

The image installs both executable names and starts `disco serve` by default. Mount configuration at `/config` as before.
