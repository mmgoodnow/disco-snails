import { listThreadSummaries, type ThreadSummaryRow } from "./db";
import type { TranscriptMessage } from "./summarizer";

function escapeHtml(input: string) {
  return input
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function renderAiSummary(summary: string) {
  const trimmed = summary?.trim();
  if (!trimmed) {
    return "<p>No AI summary available.</p>";
  }

  return trimmed;
}

function renderTranscript(transcriptJson: string) {
  try {
    const transcript = JSON.parse(transcriptJson) as TranscriptMessage[];
    if (!Array.isArray(transcript) || transcript.length === 0) {
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
  } catch (err) {
    console.error("Failed to parse transcript JSON", err);
    return `<pre>${escapeHtml(transcriptJson)}</pre>`;
  }
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

export function startServer(port: number) {
  const server = Bun.serve({
    port,
    async fetch() {
      const rows = await listThreadSummaries();
      const html = renderPage(rows);
      return new Response(html, {
        headers: { "content-type": "text/html; charset=utf-8" },
      });
    },
  });

  console.log(`Server listening on ${server.url}`);
  return server;
}
