use anyhow::Result;
use async_openai::{
    Client,
    types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub user: String,
    pub content: String,
}

pub fn format_transcript(messages: &[TranscriptMessage]) -> String {
    messages.iter().map(|m| {
        let indented = m.content.lines().map(|l| format!("\t{l}")).collect::<Vec<_>>().join("\n");
        format!("{}:\n{}\n", m.user, indented)
    }).collect()
}

pub async fn summarize_thread(title: &str, messages: &[TranscriptMessage], model: &str) -> Result<String> {
    let transcript = format_transcript(messages);
    let prompt = format!(
        "You are summarizing a Discord support thread for cross-seed, a BitTorrent cross-seeding automation tool: https://cross-seed.org.\n\nThread title: \"{title}\"\n\nConversation:\n{transcript}\nI am the primary developer and don't have time to read these threads. Summarize this thread:\n- What was the user's problem?\n- What troubleshooting was done?\n- What was the final resolution (or current status)?\n- What improvements could we make to the docs or the app that would help? (0 to 1 improvements only)\n\nReturn 3-4 CONCISE notes formatted as HTML with <h4> and <p> tags."
    );
    let client = Client::new();
    let request = CreateChatCompletionRequestArgs::default()
        .model(model)
        .messages([ChatCompletionRequestUserMessageArgs::default().content(prompt).build()?.into()])
        .build()?;
    let response = client.chat().create(request).await?;
    Ok(response.choices.into_iter().next().and_then(|c| c.message.content).unwrap_or_else(|| "(no summary)".to_string()).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_transcript_indents_multiline() {
        let msgs = vec![
            TranscriptMessage { user: "alice".to_string(), content: "hello\nworld".to_string() },
            TranscriptMessage { user: "bob".to_string(), content: "ok".to_string() },
        ];
        let t = format_transcript(&msgs);
        assert!(t.contains("alice:\n\thello\n\tworld\n"));
        assert!(t.contains("bob:\n\tok\n"));
    }

    #[test]
    fn format_transcript_empty() {
        assert_eq!(format_transcript(&[]), "");
    }
}
