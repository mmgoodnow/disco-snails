use anyhow::Result;
use async_openai::{
    types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs},
    Client,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub user: String,
    pub content: String,
}

pub fn format_transcript(messages: &[TranscriptMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let indented = m
                .content
                .lines()
                .map(|l| format!("\t{l}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("{}:\n{}\n", m.user, indented)
        })
        .collect()
}

pub async fn summarize_thread(
    title: &str,
    messages: &[TranscriptMessage],
    model: &str,
) -> Result<String> {
    summarize_thread_with_context(title, messages, model, None, SummaryFormat::Html).await
}

#[derive(Debug, Clone, Copy)]
pub enum SummaryFormat {
    Html,
    Markdown,
}

pub async fn summarize_thread_with_context(
    title: &str,
    messages: &[TranscriptMessage],
    model: &str,
    context: Option<&str>,
    format: SummaryFormat,
) -> Result<String> {
    let transcript = format_transcript(messages);
    let context = context
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("\nAdditional context:\n{value}\n"))
        .unwrap_or_default();
    let format_instruction = match format {
        SummaryFormat::Html => "Return 3-4 concise notes formatted as HTML with <h4> and <p> tags.",
        SummaryFormat::Markdown => "Return 3-4 concise Markdown bullets with short bold labels.",
    };
    let prompt = format!(
        "You are summarizing a Discord thread for someone who needs a quick, accurate briefing.\n\nThread title: \"{title}\"\n{context}\nConversation:\n{transcript}\nSummarize:\n- the main topic, question, or problem;\n- important context, decisions, or troubleshooting;\n- the resolution or current status;\n- at most one useful follow-up, if needed.\n\nDo not invent details. {format_instruction}"
    );
    let client = Client::new();
    let request = CreateChatCompletionRequestArgs::default()
        .model(model)
        .messages([ChatCompletionRequestUserMessageArgs::default()
            .content(prompt)
            .build()?
            .into()])
        .build()?;
    let response = client.chat().create(request).await?;
    Ok(response
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.content)
        .unwrap_or_else(|| "(no summary)".to_string())
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_transcript_indents_multiline() {
        let msgs = vec![
            TranscriptMessage {
                user: "alice".to_string(),
                content: "hello\nworld".to_string(),
            },
            TranscriptMessage {
                user: "bob".to_string(),
                content: "ok".to_string(),
            },
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
