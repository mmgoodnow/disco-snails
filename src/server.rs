use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::db::{ThreadSummaryRow, list_thread_summaries};
use crate::discord::DiscordClient;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<SqlitePool>,
    pub discord: Arc<DiscordClient>,
    pub web_api_key: Option<String>,
    pub forum_channel_id: Option<String>,
    pub guild_id: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handle_index))
        .route("/feed.json", get(handle_feed))
        .route("/mcp", post(handle_mcp))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

fn extract_api_key(headers: &HeaderMap, query: &str) -> Option<String> {
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        if let Ok(s) = auth.to_str() {
            if let Some(tok) = s.strip_prefix("Bearer ") {
                return Some(tok.to_string());
            }
        }
    }
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("apikey=") {
            return Some(v.to_string());
        }
    }
    None
}

fn check_auth(state: &AppState, headers: &HeaderMap, query: &str) -> Result<Option<String>, Response> {
    let provided = extract_api_key(headers, query);
    if let Some(expected) = &state.web_api_key {
        if provided.as_deref() != Some(expected.as_str()) {
            return Err((StatusCode::UNAUTHORIZED, "Unauthorized").into_response());
        }
    }
    Ok(provided)
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

pub fn strip_html(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]*>").unwrap();
    let without = re.replace_all(s, " ");
    without.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn ms_to_iso(ms: i64) -> String {
    use chrono::{DateTime, Utc};
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TranscriptMessage {
    pub user: String,
    pub content: String,
}

fn parse_transcript(json: &str) -> Vec<TranscriptMessage> {
    serde_json::from_str(json).unwrap_or_default()
}

fn thread_uri(id: &str) -> String { format!("discord://thread/{id}") }
fn channel_uri(id: &str) -> String { format!("discord://channel/{id}") }
fn message_uri(channel_id: &str, message_id: &str) -> String {
    format!("discord://channel/{channel_id}/message/{message_id}")
}

// ---------------------------------------------------------------------------
// Discord URI parsing
// ---------------------------------------------------------------------------

pub enum DiscordUri {
    Message { channel_id: String, message_id: String },
    Channel { channel_id: String },
    Thread { channel_id: String },
}

pub fn parse_discord_uri(uri: &str) -> Option<DiscordUri> {
    if let Some(rest) = uri.strip_prefix("discord://channel/") {
        if let Some((ch, msg)) = rest.split_once("/message/") {
            return Some(DiscordUri::Message { channel_id: ch.to_string(), message_id: msg.to_string() });
        }
        return Some(DiscordUri::Channel { channel_id: rest.to_string() });
    }
    if let Some(id) = uri.strip_prefix("discord://thread/") {
        return Some(DiscordUri::Thread { channel_id: id.to_string() });
    }
    if uri.chars().all(|c| c.is_ascii_digit()) && !uri.is_empty() {
        return Some(DiscordUri::Thread { channel_id: uri.to_string() });
    }
    None
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_transcript_messages(transcript: &[TranscriptMessage]) -> String {
    transcript.iter().map(|m| format!(
        "<article class=\"message\"><header>{}</header><pre>{}</pre></article>",
        escape_html(&m.user), escape_html(&m.content)
    )).collect()
}

fn render_thread(row: &ThreadSummaryRow) -> String {
    let transcript = parse_transcript(&row.transcript_json);
    let summary_html = if row.ai_summary.trim().is_empty() {
        "<p>No AI summary available.</p>".to_string()
    } else {
        row.ai_summary.clone()
    };
    let transcript_html = if transcript.is_empty() {
        "<p>No transcript captured.</p>".to_string()
    } else {
        render_transcript_messages(&transcript)
    };
    format!(
        r#"<details>
      <summary><span class="thread-title">{title}</span><span class="timestamp">{ts}</span></summary>
      <section><h3>AI Summary</h3>{summary}<h3>Transcript</h3>{transcript}</section>
    </details>"#,
        title = escape_html(&row.name),
        ts = ms_to_iso(row.last_message_timestamp),
        summary = summary_html,
        transcript = transcript_html,
    )
}

fn render_page(rows: &[ThreadSummaryRow]) -> String {
    let content = if rows.is_empty() {
        "<p>No thread summaries stored yet.</p>".to_string()
    } else {
        rows.iter().map(render_thread).collect()
    };
    format!(r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Discord Thread Summaries</title>
    <style>
      body {{ font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; margin: 0; background: #f8fafc; color: #0f172a; line-height: 1.5; }}
      .content {{ max-width: 960px; margin: 0 auto; padding: 2rem; }}
      h1 {{ font-size: 1.8rem; margin-bottom: 1.5rem; }}
      details {{ border: 1px solid #cbd5f5; border-radius: 0.5rem; margin-bottom: 1rem; background: #ffffff; overflow: hidden; }}
      summary {{ cursor: pointer; display: flex; justify-content: space-between; align-items: center; font-weight: 600; padding: 0.75rem 1rem; }}
      summary::-webkit-details-marker {{ display: none; }}
      .thread-title {{ margin-right: 1rem; }} .timestamp {{ font-size: 0.85rem; color: #475569; }}
      section {{ padding: 0 1rem 1rem; margin-top: 0.25rem; }}
      h3 {{ margin: 1rem 0 0.5rem; font-size: 1rem; color: #0f172a; }}
      .message {{ border: 1px solid #cbd5f5; border-radius: 0.5rem; padding: 0.5rem 0.75rem; margin-bottom: 0.5rem; background: #e2e8f0; }}
      .message header {{ font-weight: 600; margin-bottom: 0.25rem; }}
      pre {{ font-family: inherit; white-space: pre-wrap; word-break: break-word; margin: 0; }}
      @media (prefers-color-scheme: dark) {{
        body {{ background: #0f172a; color: #e2e8f0; }} details {{ border-color: #334155; background: #1e293b; }}
        .timestamp {{ color: #94a3b8; }} .message {{ border-color: #334155; background: #0f172a; }} h3 {{ color: #e2e8f0; }}
      }}
    </style>
  </head>
  <body><main class="content"><h1>Discord Thread Summaries</h1>{content}</main></body>
</html>"#)
}

// ---------------------------------------------------------------------------
// JSON Feed
// ---------------------------------------------------------------------------

fn build_json_feed(rows: &[ThreadSummaryRow], origin: &str, apikey: Option<&str>) -> Value {
    let feed_url = match apikey {
        Some(k) => format!("{origin}/feed.json?apikey={k}"),
        None => format!("{origin}/feed.json"),
    };
    let items: Vec<Value> = rows.iter().map(|row| {
        let item_url = match apikey {
            Some(k) => format!("{origin}/?thread={}&apikey={k}", row.snowflake),
            None => format!("{origin}/?thread={}", row.snowflake),
        };
        let transcript = parse_transcript(&row.transcript_json);
        let summary_html = row.ai_summary.trim().to_string();
        let summary_text = if summary_html.is_empty() { "No AI summary available.".to_string() } else { strip_html(&summary_html) };
        let transcript_html = if transcript.is_empty() {
            "<p>No transcript captured.</p>".to_string()
        } else {
            let items: String = transcript.iter().map(|m| format!(
                "<li><strong>{}:</strong> {}</li>", escape_html(&m.user), escape_html(&m.content)
            )).collect();
            format!("<h4>Messages</h4><ul>{items}</ul>")
        };
        let content_html = format!(
            "{}{}",
            if summary_html.is_empty() { "<p>No AI summary available.</p>".to_string() } else { summary_html.clone() },
            transcript_html
        );
        json!({
            "id": row.snowflake, "title": row.name, "url": item_url,
            "summary": summary_text, "content_html": content_html,
            "date_published": ms_to_iso(row.last_message_timestamp),
            "date_modified": ms_to_iso(row.updated_at),
            "transcript": transcript,
        })
    }).collect();
    json!({
        "version": "https://jsonfeed.org/version/1",
        "title": "Discord Thread Summaries",
        "home_page_url": origin, "feed_url": feed_url, "items": items,
    })
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: Option<String>,
    params: Option<Value>,
}

fn rpc_result(id: Option<&Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
fn rpc_error(id: Option<&Value>, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
fn mcp_text(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> { v.get(key)?.as_str() }
fn get_u64(v: &Value, key: &str) -> Option<u64> { v.get(key)?.as_u64() }
fn get_str_array(v: &Value, key: &str) -> Option<Vec<String>> {
    v.get(key)?.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

fn row_matches(row: &ThreadSummaryRow, query: &str) -> bool {
    if query.is_empty() { return true; }
    let haystack = format!("{}\n{}\n{}\n{}", row.snowflake, row.name, strip_html(&row.ai_summary), row.transcript_json).to_lowercase();
    haystack.contains(&query.to_lowercase())
}

fn render_thread_summary_markdown(row: &ThreadSummaryRow) -> String {
    let transcript = parse_transcript(&row.transcript_json);
    let messages = transcript.iter().map(|m| format!("- **{}:** {}", m.user, m.content)).collect::<Vec<_>>().join("\n");
    let summary_text = strip_html(&row.ai_summary);
    format!(
        "# {name}\n\nThread ID: {id}\nLast message: {last}\nUpdated: {updated}\n\n## AI Summary\n\n{summary}\n\n## Transcript\n\n{transcript}",
        name = row.name, id = row.snowflake,
        last = ms_to_iso(row.last_message_timestamp), updated = ms_to_iso(row.updated_at),
        summary = if summary_text.is_empty() { "No AI summary available.".to_string() } else { summary_text },
        transcript = if messages.is_empty() { "No transcript captured.".to_string() } else { messages },
    )
}

fn render_thread_summary_list(rows: &[ThreadSummaryRow]) -> String {
    let items: Vec<Value> = rows.iter().map(|row| {
        let summary = strip_html(&row.ai_summary);
        json!({
            "id": row.snowflake, "uri": thread_uri(&row.snowflake), "name": row.name,
            "summary": if summary.is_empty() { Value::Null } else { Value::String(summary) },
            "lastMessageTimestamp": ms_to_iso(row.last_message_timestamp),
            "updatedAt": ms_to_iso(row.updated_at),
        })
    }).collect();
    serde_json::to_string_pretty(&items).unwrap_or_default()
}

fn build_search_results(rows: &[ThreadSummaryRow], query: &str, limit: usize, forum_channel_id: Option<&str>) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();
    let q_lower = query.to_lowercase();
    if let Some(fid) = forum_channel_id {
        if query.is_empty() || "forum channel discord thread summaries".contains(&q_lower) || fid.contains(query) {
            results.push(json!({
                "type": "channel", "id": fid, "uri": channel_uri(fid),
                "name": "Discord forum channel", "summary": format!("{} stored thread summaries", rows.len()),
            }));
        }
    }
    for row in rows {
        if results.len() >= limit { break; }
        if !row_matches(row, query) { continue; }
        let summary = strip_html(&row.ai_summary);
        results.push(json!({
            "type": "thread", "id": row.snowflake, "uri": thread_uri(&row.snowflake), "name": row.name,
            "summary": if summary.is_empty() { Value::Null } else { Value::String(summary) },
            "lastMessageTimestamp": ms_to_iso(row.last_message_timestamp),
        }));
    }
    results.truncate(limit);
    results
}

async fn resolve_guild_id(state: &AppState) -> Option<String> {
    if let Some(g) = &state.guild_id {
        return Some(g.clone());
    }
    if let Some(fid) = &state.forum_channel_id {
        return state.discord.get_channel(fid).await.ok()?.guild_id;
    }
    None
}

async fn dispatch_mcp(
    req: JsonRpcRequest,
    rows: &[ThreadSummaryRow],
    state: &AppState,
) -> Option<Value> {
    let id = req.id.as_ref();
    let params = req.params.as_ref().unwrap_or(&Value::Null);

    match req.method.as_deref() {
        Some("initialize") => Some(rpc_result(id, json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "disco-snails", "version": "0.1.0" },
        }))),

        Some("tools/list") => Some(rpc_result(id, json!({ "tools": [
            { "name": "search", "description": "Search Discord messages, plus locally stored summarized thread metadata.",
              "inputSchema": { "type": "object", "required": ["query"], "properties": {
                  "query": { "type": "string" }, "channel_ids": { "type": "array", "items": { "type": "string" } },
                  "limit": { "type": "number" } } } },
            { "name": "read", "description": "Read live Discord messages from a channel/thread, or a single Discord message.",
              "inputSchema": { "type": "object", "required": ["uri"], "properties": {
                  "uri": { "type": "string" }, "limit": { "type": "number" } } } },
            { "name": "list_channels", "description": "List Discord channels visible to the bot.",
              "inputSchema": { "type": "object", "properties": {} } },
            { "name": "read_thread_summary", "description": "Read locally stored AI summary and captured transcript for a processed thread.",
              "inputSchema": { "type": "object", "required": ["thread_id"], "properties": {
                  "thread_id": { "type": "string" } } } },
            { "name": "list_thread_summaries", "description": "List locally stored thread summaries with thread URIs for discovery.",
              "inputSchema": { "type": "object", "properties": {
                  "query": { "type": "string" }, "limit": { "type": "number" } } } },
        ] }))),

        Some("tools/call") => {
            let name = get_str(params, "name");
            let args = params.get("arguments").unwrap_or(&Value::Null);
            match name {
                Some("list_thread_summaries") => {
                    let query = get_str(args, "query").unwrap_or("").to_string();
                    let limit = get_u64(args, "limit").map(|n| n.clamp(1, 200) as usize).unwrap_or(50);
                    let matches: Vec<ThreadSummaryRow> = rows.iter().filter(|r| row_matches(r, &query)).take(limit).cloned().collect();
                    Some(rpc_result(id, mcp_text(render_thread_summary_list(&matches))))
                }
                Some("read_thread_summary") => {
                    let thread_id = match get_str(args, "thread_id") {
                        Some(t) => t.to_string(),
                        None => return Some(rpc_error(id, -32602, "read_thread_summary requires a thread_id argument")),
                    };
                    let lookup = thread_id.strip_prefix("discord://thread/").unwrap_or(&thread_id);
                    let row = rows.iter().find(|r| r.snowflake == lookup)
                        .or_else(|| rows.iter().find(|r| r.name == thread_id));
                    match row {
                        Some(r) => Some(rpc_result(id, mcp_text(render_thread_summary_markdown(r)))),
                        None => Some(rpc_error(id, -32004, &format!("No stored thread summary found for {thread_id}"))),
                    }
                }
                Some("search") => {
                    let query = get_str(args, "query").unwrap_or("").to_string();
                    let limit = get_u64(args, "limit").map(|n| n.clamp(1, 50) as usize).unwrap_or(10);
                    let channel_ids = get_str_array(args, "channel_ids").unwrap_or_default();

                    // Live Discord search (best-effort — if we can't resolve guild, skip)
                    let discord_results: Vec<Value> = if !query.is_empty() {
                        let gid = resolve_guild_id(state).await;
                        if let Some(gid) = gid {
                            match state.discord.search_messages(&gid, &query, &channel_ids).await {
                                Ok(msgs) => msgs.iter().map(|m| {
                                    let author = m.author.as_ref().map(|a| a.display_name().to_string()).unwrap_or_else(|| "Unknown user".to_string());
                                    json!({
                                        "type": "message", "id": m.id,
                                        "uri": message_uri(&m.channel_id, &m.id),
                                        "name": format!("{author} in {}", m.channel_id),
                                        "content": m.content,
                                        "lastMessageTimestamp": m.timestamp,
                                    })
                                }).collect(),
                                Err(_) => vec![],
                            }
                        } else { vec![] }
                    } else { vec![] };

                    let stored = build_search_results(rows, &query, limit, state.forum_channel_id.as_deref());
                    let combined: Vec<Value> = discord_results.into_iter().chain(stored).take(limit).collect();
                    Some(rpc_result(id, mcp_text(serde_json::to_string_pretty(&combined).unwrap_or_default())))
                }
                Some("read") => {
                    let uri = match get_str(args, "uri") {
                        Some(u) => u.to_string(),
                        None => return Some(rpc_error(id, -32602, "read requires a uri argument")),
                    };
                    let parsed = match parse_discord_uri(&uri) {
                        Some(p) => p,
                        None => return Some(rpc_error(id, -32602, &format!("Unsupported Discord URI: {uri}"))),
                    };
                    match parsed {
                        DiscordUri::Message { channel_id, message_id } => {
                            match state.discord.get_message(&channel_id, &message_id).await {
                                Ok(msg) => {
                                    let author = msg.author.as_ref().map(|a| a.display_name().to_string()).unwrap_or_else(|| "Unknown user".to_string());
                                    let text = format!(
                                        "## Message {}\n\nChannel: {}\nAuthor: {}\nTimestamp: {}\n\n{}",
                                        msg.id, channel_uri(&msg.channel_id), author, msg.timestamp,
                                        if msg.content.is_empty() { "(empty message)" } else { &msg.content },
                                    );
                                    Some(rpc_result(id, mcp_text(text)))
                                }
                                Err(e) => Some(rpc_error(id, -32000, &e.to_string())),
                            }
                        }
                        DiscordUri::Channel { channel_id } | DiscordUri::Thread { channel_id } => {
                            // If this is the forum channel itself, list threads
                            if state.forum_channel_id.as_deref() == Some(channel_id.as_str()) {
                                if let Some(gid) = resolve_guild_id(state).await {
                                    match state.discord.get_forum_threads(&channel_id, &gid).await {
                                        Ok(threads) => {
                                            let by_id: std::collections::HashMap<_, _> = rows.iter().map(|r| (r.snowflake.as_str(), r)).collect();
                                            let lines: String = threads.iter().map(|t| {
                                                let summary = by_id.get(t.id.as_str())
                                                    .map(|r| format!("\n  Summary: {}", strip_html(&r.ai_summary)))
                                                    .unwrap_or_default();
                                                format!("- {} ({}){}", t.name, thread_uri(&t.id), summary)
                                            }).collect::<Vec<_>>().join("\n");
                                            let text = format!("# Discord Threads\n\n{}", if lines.is_empty() { "No Discord threads found.".to_string() } else { lines });
                                            return Some(rpc_result(id, mcp_text(text)));
                                        }
                                        Err(e) => return Some(rpc_error(id, -32000, &e.to_string())),
                                    }
                                }
                            }
                            // Regular channel/thread: fetch live messages + attach stored summary if any
                            let limit = get_u64(args, "limit").map(|n| n.clamp(1, 100) as u8).unwrap_or(25);
                            match state.discord.get_messages(&channel_id, limit).await {
                                Ok(messages) => {
                                    let stored = rows.iter().find(|r| r.snowflake == channel_id);
                                    let msg_text: String = messages.iter().map(|m| {
                                        let author = m.author.as_ref().map(|a| a.display_name().to_string()).unwrap_or_else(|| "Unknown user".to_string());
                                        format!("## Message {}\n\nChannel: {}\nAuthor: {}\nTimestamp: {}\n\n{}\n",
                                            m.id, channel_uri(&m.channel_id), author, m.timestamp,
                                            if m.content.is_empty() { "(empty message)" } else { &m.content })
                                    }).collect::<Vec<_>>().join("\n");
                                    let summary_section = stored.map(|r| format!("# Stored Thread Summary\n\n{}\n\n", render_thread_summary_markdown(r))).unwrap_or_default();
                                    let text = format!("# Discord Messages\n\nChannel or thread: {}\n\n{}{}",
                                        channel_uri(&channel_id), summary_section,
                                        if msg_text.is_empty() { "No Discord messages found.".to_string() } else { msg_text });
                                    Some(rpc_result(id, mcp_text(text)))
                                }
                                Err(e) => Some(rpc_error(id, -32000, &e.to_string())),
                            }
                        }
                    }
                }
                Some("list_channels") => {
                    match resolve_guild_id(state).await {
                        None => Some(rpc_error(id, -32000, "DISCORD_GUILD_ID or DISCORD_FORUM_CHANNEL_ID must be set to list channels")),
                        Some(gid) => match state.discord.get_guild_channels(&gid).await {
                            Ok(mut channels) => {
                                channels.sort_by(|a, b| a.name.as_deref().unwrap_or(&a.id).cmp(b.name.as_deref().unwrap_or(&b.id)));
                                let lines: String = channels.iter().map(|c| {
                                    let name = c.name.as_deref().unwrap_or(&c.id);
                                    let topic = c.topic.as_ref().map(|t| format!("\n  Topic: {t}")).unwrap_or_default();
                                    let parent = c.parent_id.as_ref().map(|p| format!("\n  Parent: {p}")).unwrap_or_default();
                                    format!("- {name} ({}), type {}{parent}{topic}", channel_uri(&c.id), c.kind)
                                }).collect::<Vec<_>>().join("\n");
                                let text = format!("# Discord Channels\n\n{}", if lines.is_empty() { "No channels found.".to_string() } else { lines });
                                Some(rpc_result(id, mcp_text(text)))
                            }
                            Err(e) => Some(rpc_error(id, -32000, &e.to_string())),
                        }
                    }
                }
                Some(other) => Some(rpc_error(id, -32601, &format!("Unknown tool: {other}"))),
                None => Some(rpc_error(id, -32601, "Missing tool name")),
            }
        }

        Some("notifications/initialized") => None,

        Some(other) => Some(rpc_error(id, -32601, &format!("Method not found: {other}"))),
        None => Some(rpc_error(id, -32601, "Method not found: missing")),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_index(State(state): State<AppState>, req: Request<Body>) -> Response {
    let query = req.uri().query().unwrap_or("").to_string();
    match check_auth(&state, req.headers(), &query) {
        Err(r) => r,
        Ok(_) => match list_thread_summaries(&state.pool).await {
            Ok(rows) => Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(render_page(&rows))).unwrap(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
    }
}

async fn handle_feed(State(state): State<AppState>, req: Request<Body>) -> Response {
    let query = req.uri().query().unwrap_or("").to_string();
    let origin = format!(
        "{}://{}",
        req.uri().scheme_str().unwrap_or("http"),
        req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost"),
    );
    match check_auth(&state, req.headers(), &query) {
        Err(r) => r,
        Ok(key) => match list_thread_summaries(&state.pool).await {
            Ok(rows) => {
                let feed = build_json_feed(&rows, &origin, key.as_deref());
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/feed+json; charset=utf-8")
                    .body(Body::from(serde_json::to_string_pretty(&feed).unwrap_or_default())).unwrap()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
    }
}

async fn handle_mcp(State(state): State<AppState>, req: Request<Body>) -> Response {
    let query = req.uri().query().unwrap_or("").to_string();
    if let Err(r) = check_auth(&state, req.headers(), &query) { return r; }

    let body = match axum::body::to_bytes(req.into_body(), 4 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_REQUEST, "Failed to read body").into_response(),
    };

    let json_body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            let err = rpc_error(None, -32700, "Parse error");
            return Response::builder().status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(serde_json::to_string(&err).unwrap())).unwrap();
        }
    };

    let rows = match list_thread_summaries(&state.pool).await {
        Ok(r) => r,
        Err(e) => {
            let err = rpc_error(None, -32000, &e.to_string());
            return Response::builder().header(header::CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(serde_json::to_string(&err).unwrap())).unwrap();
        }
    };

    let is_batch = json_body.is_array();
    let requests: Vec<Value> = if is_batch { json_body.as_array().cloned().unwrap_or_default() } else { vec![json_body] };

    let mut responses: Vec<Value> = Vec::new();
    for req_val in requests {
        match serde_json::from_value::<JsonRpcRequest>(req_val) {
            Ok(rpc) => {
                if let Some(resp) = dispatch_mcp(rpc, &rows, &state).await {
                    responses.push(resp);
                }
            }
            Err(e) => responses.push(rpc_error(None, -32700, &e.to_string())),
        }
    }

    if responses.is_empty() {
        return Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap();
    }

    let body_str = if is_batch { serde_json::to_string(&responses) } else { serde_json::to_string(&responses[0]) }
        .unwrap_or_default();

    Response::builder()
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(Body::from(body_str)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    use serde_json::{Value, json};
    use axum::http::{StatusCode, header};
    use std::sync::Arc;
    use crate::db::{open_db, upsert_thread_summary};
    use crate::discord::DiscordClient;
    use tempfile::NamedTempFile;

    async fn make_server() -> (TestServer, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = format!("sqlite:{}", file.path().display());
        let pool = open_db(&path).await.unwrap();
        upsert_thread_summary(
            &pool, "111", "Test Thread",
            r#"[{"user":"alice","content":"hello world"}]"#,
            "<h4>Problem</h4><p>It broke.</p>",
            1_700_000_000_000, 1_700_000_001_000,
        ).await.unwrap();
        let state = AppState {
            pool: Arc::new(pool),
            discord: Arc::new(DiscordClient::new("fake")),
            web_api_key: None,
            forum_channel_id: Some("999".to_string()),
            guild_id: None,
        };
        (TestServer::new(router(state)), file)
    }

    async fn make_server_with_key(key: &str) -> (TestServer, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = format!("sqlite:{}", file.path().display());
        let pool = open_db(&path).await.unwrap();
        let state = AppState {
            pool: Arc::new(pool),
            discord: Arc::new(DiscordClient::new("fake")),
            web_api_key: Some(key.to_string()),
            forum_channel_id: None,
            guild_id: None,
        };
        (TestServer::new(router(state)), file)
    }

    // --- pure unit tests ---

    #[test]
    fn strip_html_removes_tags() {
        assert_eq!(strip_html("<h4>hello</h4><p>world</p>"), "hello world");
    }

    #[test]
    fn strip_html_empty() {
        assert_eq!(strip_html(""), "");
    }

    #[test]
    fn escape_html_escapes_all_chars() {
        assert_eq!(escape_html("<b>\"hello\" & 'world'</b>"), "&lt;b&gt;&quot;hello&quot; &amp; &#39;world&#39;&lt;/b&gt;");
    }

    #[test]
    fn parse_discord_uri_message() {
        let u = parse_discord_uri("discord://channel/123/message/456").unwrap();
        assert!(matches!(u, DiscordUri::Message { ref channel_id, ref message_id } if channel_id == "123" && message_id == "456"));
    }

    #[test]
    fn parse_discord_uri_channel() {
        let u = parse_discord_uri("discord://channel/123").unwrap();
        assert!(matches!(u, DiscordUri::Channel { ref channel_id } if channel_id == "123"));
    }

    #[test]
    fn parse_discord_uri_thread() {
        let u = parse_discord_uri("discord://thread/789").unwrap();
        assert!(matches!(u, DiscordUri::Thread { ref channel_id } if channel_id == "789"));
    }

    #[test]
    fn parse_discord_uri_raw_snowflake() {
        let u = parse_discord_uri("123456789").unwrap();
        assert!(matches!(u, DiscordUri::Thread { .. }));
    }

    #[test]
    fn parse_discord_uri_invalid() {
        assert!(parse_discord_uri("https://example.com").is_none());
    }

    #[test]
    fn ms_to_iso_roundtrips() {
        // 2023-11-14T22:13:20Z in ms
        assert_eq!(ms_to_iso(1_700_000_000_000), "2023-11-14T22:13:20+00:00");
    }

    // --- HTTP tests ---

    #[tokio::test]
    async fn index_returns_html_with_thread() {
        let (server, _f) = make_server().await;
        let resp = server.get("/").await;
        resp.assert_status_ok();
        assert!(resp.headers()["content-type"].to_str().unwrap().contains("text/html"));
        let text = resp.text();
        assert!(text.contains("Discord Thread Summaries"));
        assert!(text.contains("Test Thread"));
        assert!(text.contains("It broke."));
    }

    #[tokio::test]
    async fn index_requires_auth_when_key_set() {
        let (server, _f) = make_server_with_key("secret").await;
        server.get("/").await.assert_status(StatusCode::UNAUTHORIZED);
        server.get("/?apikey=secret").await.assert_status_ok();
        server.get("/").add_header(header::AUTHORIZATION, "Bearer secret").await.assert_status_ok();
    }

    #[tokio::test]
    async fn feed_returns_jsonfeed_shape() {
        let (server, _f) = make_server().await;
        let resp = server.get("/feed.json").await;
        resp.assert_status_ok();
        assert!(resp.headers()["content-type"].to_str().unwrap().contains("application/feed+json"));
        let body: Value = resp.json();
        assert_eq!(body["version"], "https://jsonfeed.org/version/1");
        assert_eq!(body["title"], "Discord Thread Summaries");
        let items = body["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "111");
        assert_eq!(items[0]["title"], "Test Thread");
        assert!(items[0]["content_html"].as_str().unwrap().contains("It broke."));
        let transcript = items[0]["transcript"].as_array().unwrap();
        assert_eq!(transcript[0]["user"], "alice");
    }

    #[tokio::test]
    async fn mcp_initialize() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }))
            .await;
        resp.assert_status_ok();
        let body: Value = resp.json();
        assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(body["result"]["serverInfo"]["name"], "disco-snails");
    }

    #[tokio::test]
    async fn mcp_tools_list_has_all_five() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .await;
        let body: Value = resp.json();
        let names: Vec<&str> = body["result"]["tools"].as_array().unwrap()
            .iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"read"));
        assert!(names.contains(&"list_channels"));
        assert!(names.contains(&"read_thread_summary"));
        assert!(names.contains(&"list_thread_summaries"));
    }

    #[tokio::test]
    async fn mcp_list_thread_summaries() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "list_thread_summaries", "arguments": {} } }))
            .await;
        let body: Value = resp.json();
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        let items: Vec<Value> = serde_json::from_str(text).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "111");
        assert_eq!(items[0]["name"], "Test Thread");
    }

    #[tokio::test]
    async fn mcp_read_thread_summary_by_id() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "read_thread_summary", "arguments": { "thread_id": "111" } } }))
            .await;
        let body: Value = resp.json();
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Test Thread"));
        assert!(text.contains("It broke."));
        assert!(text.contains("alice"));
    }

    #[tokio::test]
    async fn mcp_read_thread_summary_not_found() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": { "name": "read_thread_summary", "arguments": { "thread_id": "999" } } }))
            .await;
        let body: Value = resp.json();
        assert!(body["error"].is_object());
        assert_eq!(body["error"]["code"], -32004);
    }

    #[tokio::test]
    async fn mcp_search_local_match() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/call",
                "params": { "name": "search", "arguments": { "query": "Test Thread" } } }))
            .await;
        let body: Value = resp.json();
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        let results: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(results.iter().any(|r| r["name"] == "Test Thread"));
    }

    #[tokio::test]
    async fn mcp_search_no_match() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": { "name": "search", "arguments": { "query": "zzznomatch" } } }))
            .await;
        let body: Value = resp.json();
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        let results: Vec<Value> = serde_json::from_str(text).unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn mcp_notification_returns_202() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
        resp.assert_status(StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn mcp_unknown_method_returns_error() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!({ "jsonrpc": "2.0", "id": 8, "method": "no/such/method" }))
            .await;
        let body: Value = resp.json();
        assert_eq!(body["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn mcp_batch_requests() {
        let (server, _f) = make_server().await;
        let resp = server.post("/mcp")
            .json(&json!([
                { "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} },
                { "jsonrpc": "2.0", "id": 2, "method": "tools/list" },
            ]))
            .await;
        let body: Value = resp.json();
        assert_eq!(body.as_array().unwrap().len(), 2);
    }
}
