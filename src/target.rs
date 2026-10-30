use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageTarget {
    pub guild_id: Option<String>,
    pub channel_id: String,
    pub message_id: String,
}

pub fn parse_channel_id(input: &str) -> Result<String> {
    let input = input.trim().trim_end_matches('/');
    if is_snowflake(input) {
        return Ok(input.to_string());
    }

    if let Some(rest) = input.strip_prefix("discord://channel/") {
        let channel_id = rest.split('/').next().unwrap_or_default();
        return require_snowflake(channel_id, "channel ID");
    }

    if let Some(parts) = discord_web_parts(input) {
        return require_snowflake(parts.channel_id, "channel ID");
    }

    bail!("expected a Discord channel/thread ID or discord.com channel URL, got {input:?}")
}

pub fn parse_message_target(input: &str) -> Result<MessageTarget> {
    let input = input.trim().trim_end_matches('/');
    if let Some(rest) = input.strip_prefix("discord://channel/") {
        let (channel_id, message_id) = rest
            .split_once("/message/")
            .context("Discord message URI must include /message/<id>")?;
        return Ok(MessageTarget {
            guild_id: None,
            channel_id: require_snowflake(channel_id, "channel ID")?,
            message_id: require_snowflake(message_id, "message ID")?,
        });
    }

    if let Some(parts) = discord_web_parts(input) {
        let message_id = parts
            .message_id
            .context("Discord message URL must include a message ID")?;
        return Ok(MessageTarget {
            guild_id: if parts.guild_id == "@me" {
                None
            } else {
                Some(require_snowflake(parts.guild_id, "guild ID")?)
            },
            channel_id: require_snowflake(parts.channel_id, "channel ID")?,
            message_id: require_snowflake(message_id, "message ID")?,
        });
    }

    bail!("expected a Discord message URL or discord:// message URI, got {input:?}")
}

pub fn message_target_from_ids(channel: &str, message: &str) -> Result<MessageTarget> {
    Ok(MessageTarget {
        guild_id: None,
        channel_id: parse_channel_id(channel)?,
        message_id: require_snowflake(message, "message ID")?,
    })
}

struct WebParts<'a> {
    guild_id: &'a str,
    channel_id: &'a str,
    message_id: Option<&'a str>,
}

fn discord_web_parts(input: &str) -> Option<WebParts<'_>> {
    let (_, path) = input.split_once("://")?;
    let (host, path) = path.split_once('/')?;
    if !matches!(
        host,
        "discord.com" | "www.discord.com" | "discordapp.com" | "www.discordapp.com"
    ) {
        return None;
    }
    let rest = path.strip_prefix("channels/")?;
    let mut parts = rest.split('/');
    let guild_id = parts.next()?;
    let channel_id = parts.next()?;
    let message_id = parts.next().filter(|part| !part.is_empty());
    Some(WebParts {
        guild_id,
        channel_id,
        message_id,
    })
}

fn is_snowflake(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn require_snowflake(value: &str, label: &str) -> Result<String> {
    if is_snowflake(value) {
        Ok(value.to_string())
    } else {
        bail!("invalid {label}: {value:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_channel_id_and_url() {
        assert_eq!(parse_channel_id("123").unwrap(), "123");
        assert_eq!(
            parse_channel_id("https://discord.com/channels/111/222/333").unwrap(),
            "222"
        );
        assert_eq!(parse_channel_id("discord://channel/444").unwrap(), "444");
    }

    #[test]
    fn parses_message_url_and_uri() {
        assert_eq!(
            parse_message_target("https://discord.com/channels/111/222/333").unwrap(),
            MessageTarget {
                guild_id: Some("111".to_string()),
                channel_id: "222".to_string(),
                message_id: "333".to_string(),
            }
        );
        assert_eq!(
            parse_message_target("discord://channel/222/message/333").unwrap(),
            MessageTarget {
                guild_id: None,
                channel_id: "222".to_string(),
                message_id: "333".to_string(),
            }
        );
    }

    #[test]
    fn rejects_non_discord_targets() {
        assert!(parse_channel_id("https://example.com/channels/1/2").is_err());
        assert!(parse_message_target("333").is_err());
    }
}
