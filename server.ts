import { listThreadSummaries, type ThreadSummaryRow } from "./db";
import type { TranscriptMessage } from "./summarizer";

const WEB_API_KEY = process.env.WEB_API_KEY;

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
