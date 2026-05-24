import { listThreadSummaries, type ThreadSummaryRow } from "./db";
import type { TranscriptMessage } from "./summarizer";

const WEB_API_KEY = process.env.WEB_API_KEY;
const FORUM_CHANNEL_ID = process.env.DISCORD_FORUM_CHANNEL_ID;
const DISCORD_GUILD_ID = process.env.DISCORD_GUILD_ID;
const DISCORD_BOT_TOKEN = process.env.DISCORD_BOT_TOKEN;
const MCP_PROTOCOL_VERSION = "2025-06-18";
const DISCORD_API_BASE = "https://discord.com/api/v10";

type JsonRpcRequest = {
  jsonrpc?: string;
  id?: string | number | null;
  method?: string;
  params?: unknown;
};

type McpContent = {
  type: "text";
  text: string;
};

type SearchResult = {
  type: "channel" | "thread" | "message";
  id: string;
  uri: string;
  name: string;
  content?: string;
  summary?: string;
  lastMessageTimestamp?: string;
};

type DiscordMessage = {
  id: string;
  channel_id: string;
  content: string;
  timestamp: string;
  author?: {
    id: string;
    username: string;
    global_name?: string | null;
  };
};

type DiscordThread = {
  id: string;
  name: string;
  parent_id?: string | null;
  thread_metadata?: {
    archived?: boolean;
    archive_timestamp?: string;
  };
};

type DiscordChannel = {
  id: string;
  name?: string;
  type: number;
  parent_id?: string | null;
  topic?: string | null;
};

function parseTranscript(transcriptJson: string): TranscriptMessage[] {
  try {
    const parsed = JSON.parse(transcriptJson);
    return Array.isArray(parsed) ? parsed : [];
  } catch (err) {
    console.error("Failed to parse transcript JSON", err);
    return [];
  }
}

function escapeHtml(input: string) {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function stripHtml(input: string) {
  if (!input) return "";
  const withoutTags = input.replace(/<[^>]*>/g, " ");
  return withoutTags.replace(/\s+/g, " ").trim();
}

function getStringParam(params: unknown, key: string) {
  if (!params || typeof params !== "object") return undefined;
  const value = (params as Record<string, unknown>)[key];
  return typeof value === "string" ? value : undefined;
}

function getNumberParam(params: unknown, key: string) {
  if (!params || typeof params !== "object") return undefined;
  const value = (params as Record<string, unknown>)[key];
  return typeof value === "number" && Number.isFinite(value)
    ? value
    : undefined;
}

function extractToolArguments(params: unknown) {
  if (!params || typeof params !== "object") return undefined;
  const args = (params as Record<string, unknown>).arguments;
  return args && typeof args === "object" ? args : undefined;
}

function getStringArrayParam(params: unknown, key: string) {
  if (!params || typeof params !== "object") return undefined;
  const value = (params as Record<string, unknown>)[key];
  if (!Array.isArray(value)) return undefined;
  return value.filter((item): item is string => typeof item === "string");
}

function jsonRpcResult(id: JsonRpcRequest["id"], result: unknown) {
  return { jsonrpc: "2.0", id, result };
}

function jsonRpcError(id: JsonRpcRequest["id"], code: number, message: string) {
  return { jsonrpc: "2.0", id, error: { code, message } };
}

function mcpText(text: string): McpContent[] {
  return [{ type: "text", text }];
}

function threadUri(snowflake: string) {
  return `discord://thread/${snowflake}`;
}

function channelUri(channelId: string) {
  return `discord://channel/${channelId}`;
}

function messageUri(channelId: string, messageId: string) {
  return `discord://channel/${channelId}/message/${messageId}`;
}

function parseDiscordUri(uri: string) {
  const channelMessageMatch = uri.match(
    /^discord:\/\/channel\/(\d+)\/message\/(\d+)$/,
  );
  if (channelMessageMatch) {
    return {
      type: "message" as const,
      channelId: channelMessageMatch[1],
      messageId: channelMessageMatch[2],
    };
  }

  const channelMatch = uri.match(/^discord:\/\/channel\/(\d+)$/);
  if (channelMatch) {
    return { type: "channel" as const, channelId: channelMatch[1] };
  }

  const threadMatch = uri.match(/^discord:\/\/thread\/(\d+)$/);
  if (threadMatch) {
    return { type: "thread" as const, channelId: threadMatch[1] };
  }

  if (/^\d+$/.test(uri)) {
    return { type: "thread" as const, channelId: uri };
  }

  return undefined;
}

async function discordApi<T>(path: string): Promise<T> {
  if (!DISCORD_BOT_TOKEN) {
    throw new Error("DISCORD_BOT_TOKEN is not set");
  }

  const response = await fetch(`${DISCORD_API_BASE}${path}`, {
    headers: {
      authorization: `Bot ${DISCORD_BOT_TOKEN}`,
      accept: "application/json",
    },
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Discord API ${response.status}: ${text}`);
  }

  return response.json() as Promise<T>;
}

async function readDiscordMessages(channelId: string, limit: number) {
  const params = new URLSearchParams({ limit: String(limit) });
  return discordApi<DiscordMessage[]>(
    `/channels/${channelId}/messages?${params.toString()}`,
  );
}

async function readDiscordMessage(channelId: string, messageId: string) {
  return discordApi<DiscordMessage>(
    `/channels/${channelId}/messages/${messageId}`,
  );
}

async function readDiscordForumThreads(channelId: string) {
  const activePromise = DISCORD_GUILD_ID
    ? discordApi<{ threads: DiscordThread[] }>(
        `/guilds/${DISCORD_GUILD_ID}/threads/active`,
      )
    : Promise.resolve({ threads: [] });
  const archivedPromise = discordApi<{ threads: DiscordThread[] }>(
    `/channels/${channelId}/threads/archived/public?limit=100`,
  );

  const [active, archived] = await Promise.all([
    activePromise,
    archivedPromise,
  ]);
  const activeThreads = active.threads.filter(
    (thread) => thread.parent_id === channelId,
  );

  return [...activeThreads, ...archived.threads];
}

async function listDiscordChannels() {
  if (!DISCORD_GUILD_ID) {
    throw new Error("DISCORD_GUILD_ID is not set");
  }

  return discordApi<DiscordChannel[]>(`/guilds/${DISCORD_GUILD_ID}/channels`);
}

async function searchDiscordMessages(query: string, channelIds?: string[]) {
  if (!DISCORD_GUILD_ID) return [];

  const params = new URLSearchParams({ content: query });
  for (const channelId of channelIds ?? []) {
    params.append("channel_id", channelId);
  }

  const result = await discordApi<{ messages?: DiscordMessage[][] }>(
    `/guilds/${DISCORD_GUILD_ID}/messages/search?${params.toString()}`,
  );

  return (result.messages ?? []).flat();
}

function rowMatches(row: ThreadSummaryRow, query: string) {
  if (!query) return true;
  const haystack = [
    row.snowflake,
    row.name,
    stripHtml(row.aiSummary),
    row.transcriptJson,
  ]
    .join("\n")
    .toLowerCase();

  return haystack.includes(query.toLowerCase());
}

function buildSearchResults(
  rows: ThreadSummaryRow[],
  query: string,
  limit: number,
): SearchResult[] {
  const results: SearchResult[] = [];
  const normalizedQuery = query.toLowerCase();

  if (
    FORUM_CHANNEL_ID &&
    (!query ||
      "forum channel discord thread summaries"
        .toLowerCase()
        .includes(normalizedQuery) ||
      FORUM_CHANNEL_ID.includes(query))
  ) {
    results.push({
      type: "channel",
      id: FORUM_CHANNEL_ID,
      uri: channelUri(FORUM_CHANNEL_ID),
      name: "Discord forum channel",
      summary: `${rows.length} stored thread summaries`,
    });
  }

  for (const row of rows) {
    if (!rowMatches(row, query)) continue;
    results.push({
      type: "thread",
      id: row.snowflake,
      uri: threadUri(row.snowflake),
      name: row.name,
      summary: stripHtml(row.aiSummary) || undefined,
      lastMessageTimestamp: new Date(row.lastMessageTimestamp).toISOString(),
    });

    if (results.length >= limit) break;
  }

  return results.slice(0, limit);
}

function renderStoredThreadSummaryMarkdown(row: ThreadSummaryRow) {
  const transcript = parseTranscript(row.transcriptJson);
  const messages = transcript
    .map((entry) => `- **${entry.user}:** ${entry.content ?? ""}`)
    .join("\n");

  return [
    `# ${row.name}`,
    "",
    `Thread ID: ${row.snowflake}`,
    `Last message: ${new Date(row.lastMessageTimestamp).toISOString()}`,
    `Updated: ${new Date(row.updatedAt).toISOString()}`,
    "",
    "## AI Summary",
    "",
    stripHtml(row.aiSummary) || "No AI summary available.",
    "",
    "## Transcript",
    "",
    messages || "No transcript captured.",
  ].join("\n");
}

function renderMessageMarkdown(message: DiscordMessage) {
  const author =
    message.author?.global_name ?? message.author?.username ?? "Unknown user";
  return [
    `## Message ${message.id}`,
    "",
    `Channel: ${channelUri(message.channel_id)}`,
    `Author: ${author}`,
    `Timestamp: ${message.timestamp}`,
    "",
    message.content || "(empty message)",
  ].join("\n");
}

function renderMessagesMarkdown(
  channelId: string,
  messages: DiscordMessage[],
  storedSummary?: ThreadSummaryRow,
) {
  const renderedMessages = messages
    .map((message) => renderMessageMarkdown(message))
    .join("\n\n");
  const summary = storedSummary
    ? [
        "# Stored Thread Summary",
        "",
        renderStoredThreadSummaryMarkdown(storedSummary),
        "",
      ]
    : [];

  return [
    `# Discord Messages`,
    "",
    `Channel or thread: ${channelUri(channelId)}`,
    "",
    ...summary,
    renderedMessages || "No Discord messages found.",
  ].join("\n");
}

function renderThreadsMarkdown(
  threads: DiscordThread[],
  rows: ThreadSummaryRow[],
) {
  const summariesById = new Map(rows.map((row) => [row.snowflake, row]));
  const renderedThreads = threads
    .map((thread) => {
      const storedSummary = summariesById.get(thread.id);
      const summaryText = storedSummary
        ? `\n  Summary: ${stripHtml(storedSummary.aiSummary) || "No AI summary available."}`
        : "";
      return `- ${thread.name} (${threadUri(thread.id)})${summaryText}`;
    })
    .join("\n");

  return [
    `# Discord Threads`,
    "",
    renderedThreads || "No Discord threads found.",
  ].join("\n");
}

function renderChannelsMarkdown(channels: DiscordChannel[]) {
  const renderedChannels = channels
    .sort((a, b) => (a.name ?? a.id).localeCompare(b.name ?? b.id))
    .map((channel) => {
      const name = channel.name ?? channel.id;
      const topic = channel.topic ? `\n  Topic: ${channel.topic}` : "";
      const parent = channel.parent_id
        ? `\n  Parent: ${channel.parent_id}`
        : "";
      return `- ${name} (${channelUri(channel.id)}), type ${channel.type}${parent}${topic}`;
    })
    .join("\n");

  return [
    "# Discord Channels",
    "",
    renderedChannels || "No channels found.",
  ].join("\n");
}

function discordMessagesToSearchResults(
  messages: DiscordMessage[],
): SearchResult[] {
  return messages.map((message) => {
    const author =
      message.author?.global_name ?? message.author?.username ?? "Unknown user";
    return {
      type: "message",
      id: message.id,
      uri: messageUri(message.channel_id, message.id),
      name: `${author} in ${message.channel_id}`,
      content: message.content,
      lastMessageTimestamp: message.timestamp,
    };
  });
}

function findThreadByReadTarget(rows: ThreadSummaryRow[], target: string) {
  const threadPrefix = "discord://thread/";
  const id = target.startsWith(threadPrefix)
    ? target.slice(threadPrefix.length)
    : target;

  return rows.find((row) => row.snowflake === id);
}

async function handleMcp(request: JsonRpcRequest, rows: ThreadSummaryRow[]) {
  const { id, method, params } = request;

  switch (method) {
    case "initialize":
      return jsonRpcResult(id, {
        protocolVersion: MCP_PROTOCOL_VERSION,
        capabilities: {
          tools: {},
        },
        serverInfo: {
          name: "disco-snails",
          version: "0.1.0",
        },
      });

    case "tools/list":
      return jsonRpcResult(id, {
        tools: [
          {
            name: "search",
            description:
              "Search Discord messages, plus locally stored summarized thread metadata.",
            inputSchema: {
              type: "object",
              properties: {
                query: {
                  type: "string",
                  description:
                    "Text to search for in Discord messages and summarized thread content.",
                },
                channel_ids: {
                  type: "array",
                  items: { type: "string" },
                  description:
                    "Optional Discord channel IDs to restrict search.",
                },
                limit: {
                  type: "number",
                  description: "Maximum number of results to return.",
                },
              },
              required: ["query"],
            },
          },
          {
            name: "read",
            description:
              "Read live Discord messages from a channel/thread, or a single Discord message.",
            inputSchema: {
              type: "object",
              properties: {
                uri: {
                  type: "string",
                  description:
                    "A discord://channel/{id}, discord://thread/{id}, discord://channel/{id}/message/{id}, or raw channel/thread id.",
                },
                limit: {
                  type: "number",
                  description: "Maximum number of messages to read.",
                },
              },
              required: ["uri"],
            },
          },
          {
            name: "list_channels",
            description: "List Discord channels visible to the bot.",
            inputSchema: {
              type: "object",
              properties: {},
            },
          },
          {
            name: "read_thread_summary",
            description:
              "Read locally stored AI summary and captured transcript for a processed thread.",
            inputSchema: {
              type: "object",
              properties: {
                thread_id: {
                  type: "string",
                  description:
                    "A Discord thread ID, discord://thread/{id}, or thread name.",
                },
              },
              required: ["thread_id"],
            },
          },
        ],
      });

    case "tools/call": {
      const name = getStringParam(params, "name");
      const args = extractToolArguments(params);

      if (name === "search") {
        const query = getStringParam(args, "query") ?? "";
        const limit = Math.max(
          1,
          Math.min(getNumberParam(args, "limit") ?? 10, 50),
        );
        const channelIds = getStringArrayParam(args, "channel_ids");
        const discordMatches = query
          ? discordMessagesToSearchResults(
              await searchDiscordMessages(query, channelIds),
            )
          : [];
        const storedMatches = buildSearchResults(rows, query, limit);
        return jsonRpcResult(id, {
          content: mcpText(
            JSON.stringify(
              [...discordMatches, ...storedMatches].slice(0, limit),
              null,
              2,
            ),
          ),
        });
      }

      if (name === "read") {
        const uri = getStringParam(args, "uri");
        if (!uri) {
          return jsonRpcError(id, -32602, "read requires a uri argument");
        }

        const parsedUri = parseDiscordUri(uri);
        if (!parsedUri) {
          return jsonRpcError(id, -32602, `Unsupported Discord URI: ${uri}`);
        }

        if (parsedUri.type === "message") {
          const message = await readDiscordMessage(
            parsedUri.channelId,
            parsedUri.messageId,
          );
          return jsonRpcResult(id, {
            content: mcpText(renderMessageMarkdown(message)),
          });
        }

        if (FORUM_CHANNEL_ID && parsedUri.channelId === FORUM_CHANNEL_ID) {
          const threads = await readDiscordForumThreads(parsedUri.channelId);
          return jsonRpcResult(id, {
            content: mcpText(renderThreadsMarkdown(threads, rows)),
          });
        }

        const limit = Math.max(
          1,
          Math.min(getNumberParam(args, "limit") ?? 25, 100),
        );
        const messages = await readDiscordMessages(parsedUri.channelId, limit);
        const storedSummary =
          parsedUri.type === "thread"
            ? findThreadByReadTarget(rows, parsedUri.channelId)
            : undefined;
        return jsonRpcResult(id, {
          content: mcpText(
            renderMessagesMarkdown(
              parsedUri.channelId,
              messages,
              storedSummary,
            ),
          ),
        });
      }

      if (name === "list_channels") {
        const channels = await listDiscordChannels();
        return jsonRpcResult(id, {
          content: mcpText(renderChannelsMarkdown(channels)),
        });
      }

      if (name === "read_thread_summary") {
        const threadId = getStringParam(args, "thread_id");
        if (!threadId) {
          return jsonRpcError(
            id,
            -32602,
            "read_thread_summary requires a thread_id argument",
          );
        }

        const row =
          findThreadByReadTarget(rows, threadId) ??
          rows.find((candidate) => candidate.name === threadId);

        if (!row) {
          return jsonRpcError(
            id,
            -32004,
            `No stored thread summary found for ${threadId}`,
          );
        }

        return jsonRpcResult(id, {
          content: mcpText(renderStoredThreadSummaryMarkdown(row)),
        });
      }

      return jsonRpcError(id, -32601, `Unknown tool: ${name ?? "missing"}`);
    }

    case "notifications/initialized":
      return undefined;

    default:
      return jsonRpcError(
        id,
        -32601,
        `Method not found: ${method ?? "missing"}`,
      );
  }
}

async function handleMcpRequest(req: Request, rows: ThreadSummaryRow[]) {
  if (req.method !== "POST") {
    return new Response("Method Not Allowed", {
      status: 405,
      headers: { allow: "POST" },
    });
  }

  let body: JsonRpcRequest | JsonRpcRequest[];
  try {
    body = await req.json();
  } catch {
    return new Response(
      JSON.stringify(jsonRpcError(null, -32700, "Parse error")),
      {
        status: 400,
        headers: { "content-type": "application/json; charset=utf-8" },
      },
    );
  }

  const requests = Array.isArray(body) ? body : [body];
  const responses = (
    await Promise.all(
      requests.map(async (request) => {
        try {
          return await handleMcp(request, rows);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          return jsonRpcError(request.id, -32000, message);
        }
      }),
    )
  ).filter((response) => response !== undefined);

  if (responses.length === 0) {
    return new Response(null, { status: 202 });
  }

  return new Response(
    JSON.stringify(Array.isArray(body) ? responses : responses[0]),
    {
      headers: { "content-type": "application/json; charset=utf-8" },
    },
  );
}

function renderTranscriptHtmlForFeed(transcript: TranscriptMessage[]) {
  if (transcript.length === 0) return "<p>No transcript captured.</p>";

  const list = transcript
    .map(
      (entry) =>
        `<li><strong>${escapeHtml(entry.user)}:</strong> ${escapeHtml(entry.content ?? "")}</li>`,
    )
    .join("");

  return `<h4>Messages</h4><ul>${list}</ul>`;
}

function renderAiSummary(summary: string) {
  const trimmed = summary?.trim();
  if (!trimmed) {
    return "<p>No AI summary available.</p>";
  }

  return trimmed;
}

function renderTranscript(transcriptJson: string) {
  const transcript = parseTranscript(transcriptJson);
  if (transcript.length === 0) {
    return "<p>No transcript captured.</p>";
  }

  return transcript
    .map(
      (entry) => `
        <article class="message">
          <header>${escapeHtml(entry.user)}</header>
          <pre>${escapeHtml(entry.content ?? "")}</pre>
        </article>
      `,
    )
    .join("");
}

function renderThread(row: ThreadSummaryRow) {
  const lastUpdated = new Date(row.lastMessageTimestamp).toISOString();
  return `
    <details>
      <summary>
        <span class="thread-title">${escapeHtml(row.name)}</span>
        <span class="timestamp">${lastUpdated}</span>
      </summary>
      <section>
        <h3>AI Summary</h3>
        ${renderAiSummary(row.aiSummary)}
        <h3>Transcript</h3>
        ${renderTranscript(row.transcriptJson)}
      </section>
    </details>
  `;
}

function renderPage(rows: ThreadSummaryRow[]) {
  const content =
    rows.length === 0
      ? "<p>No thread summaries stored yet.</p>"
      : rows.map(renderThread).join("\n");

  return `<!doctype html>
  <html lang="en">
    <head>
      <meta charset="utf-8" />
      <title>Discord Thread Summaries</title>
      <style>
        body {
          font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI",
            sans-serif;
          margin: 0;
          background: #f8fafc;
          color: #0f172a;
          line-height: 1.5;
        }
        .content {
          max-width: 960px;
          margin: 0 auto;
          padding: 2rem;
        }
        h1 {
          font-size: 1.8rem;
          margin-bottom: 1.5rem;
        }
        details {
          border: 1px solid #cbd5f5;
          border-radius: 0.5rem;
          margin-bottom: 1rem;
          background: #ffffff;
          overflow: hidden;
        }
        summary {
          cursor: pointer;
          display: flex;
          justify-content: space-between;
          align-items: center;
          font-weight: 600;
          padding: 0.75rem 1rem;
        }
        summary::-webkit-details-marker {
          display: none;
        }
        .thread-title {
          margin-right: 1rem;
        }
        .timestamp {
          font-size: 0.85rem;
          color: #475569;
        }
        section {
          padding: 0 1rem 1rem;
          margin-top: 0.25rem;
        }
        h3 {
          margin: 1rem 0 0.5rem;
          font-size: 1rem;
          color: #0f172a;
        }
        ul {
          padding-left: 1.25rem;
          margin: 0.25rem 0 0.75rem;
        }
        li {
          margin-bottom: 0.4rem;
        }
        .message {
          border: 1px solid #cbd5f5;
          border-radius: 0.5rem;
          padding: 0.5rem 0.75rem;
          margin-bottom: 0.5rem;
          background: #e2e8f0;
        }
        .message header {
          font-weight: 600;
          margin-bottom: 0.25rem;
        }
        pre {
          font-family: inherit;
          white-space: pre-wrap;
          word-break: break-word;
          margin: 0;
        }
        @media (prefers-color-scheme: dark) {
          body {
            background: #0f172a;
            color: #e2e8f0;
          }
          details {
            border-color: #334155;
            background: #1e293b;
          }
          .timestamp {
            color: #94a3b8;
          }
          .message {
            border-color: #334155;
            background: #0f172a;
          }
          h3 {
            color: #e2e8f0;
          }
        }
      </style>
    </head>
    <body>
      <main class="content">
        <h1>Discord Thread Summaries</h1>
        ${content}
      </main>
    </body>
  </html>`;
}

function buildJsonFeed(
  rows: ThreadSummaryRow[],
  origin: string,
  apikey?: string | null,
) {
  const feedUrlParams = new URLSearchParams();
  if (apikey) feedUrlParams.set("apikey", apikey);

  const feedUrl = feedUrlParams.size
    ? `${origin}/feed.json?${feedUrlParams.toString()}`
    : `${origin}/feed.json`;

  return {
    version: "https://jsonfeed.org/version/1",
    title: "Discord Thread Summaries",
    home_page_url: origin,
    feed_url: feedUrl,
    items: rows.map((row) => {
      const itemParams = new URLSearchParams({ thread: row.snowflake });
      if (apikey) itemParams.set("apikey", apikey);

      const summaryHtml = row.aiSummary?.trim();
      const summaryText = stripHtml(summaryHtml);
      const transcript = parseTranscript(row.transcriptJson);
      const transcriptHtml = renderTranscriptHtmlForFeed(transcript);
      const summaryTextBlock = summaryText || "No AI summary available.";
      const summaryHtmlBlock = summaryHtml || "<p>No AI summary available.</p>";
      return {
        id: row.snowflake,
        title: row.name,
        url: `${origin}/?${itemParams.toString()}`,
        summary: summaryTextBlock,
        content_html: `${summaryHtmlBlock}${transcriptHtml}`,
        date_published: new Date(row.lastMessageTimestamp).toISOString(),
        date_modified: new Date(row.updatedAt).toISOString(),
        transcript,
      };
    }),
  };
}

export function startServer(port: number) {
  const server = Bun.serve({
    port,
    async fetch(req) {
      const url = new URL(req.url);
      const { pathname, searchParams, origin } = url;

      const providedKey = searchParams.get("apikey");

      if (WEB_API_KEY) {
        if (providedKey !== WEB_API_KEY) {
          return new Response("Unauthorized", { status: 401 });
        }
      }

      const rows = await listThreadSummaries();

      if (pathname === "/mcp") {
        return handleMcpRequest(req, rows);
      }

      if (pathname === "/feed.json") {
        const feed = buildJsonFeed(rows, origin, providedKey);
        return new Response(JSON.stringify(feed, null, 2), {
          headers: { "content-type": "application/feed+json; charset=utf-8" },
        });
      }

      const html = renderPage(rows);
      return new Response(html, {
        headers: { "content-type": "text/html; charset=utf-8" },
      });
    },
  });

  console.log(`Server listening on ${server.url}`);
  return server;
}
