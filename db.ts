import { SQL } from "bun";

export interface ThreadSummaryRow {
  snowflake: string;
  name: string;
  aiSummary: string;
  lastMessageTimestamp: number;
  updatedAt: number;
  transcriptJson: string;
}

const db = new SQL("sqlite://snails.db");

await db`
  CREATE TABLE IF NOT EXISTS thread_summaries (
    snowflake TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    transcriptJson TEXT NOT NULL,
    aiSummary TEXT NOT NULL,
    lastMessageTimestamp INTEGER NOT NULL,
    updatedAt INTEGER NOT NULL
  )
`;

export async function upsertThreadSummary(
  snowflake: string,
  name: string,
  transcriptJson: string,
  aiSummary: string,
  lastMessageTimestamp: number,
) {
  await db`
    INSERT INTO thread_summaries (
      snowflake,
      name,
      transcriptJson,
      aiSummary,
      lastMessageTimestamp,
      updatedAt
    )
    VALUES (
      ${snowflake},
      ${name},
      ${transcriptJson},
      ${aiSummary},
      ${lastMessageTimestamp},
      ${Date.now()}
    )
    ON CONFLICT(snowflake) DO UPDATE SET
      name = excluded.name,
      transcriptJson = excluded.transcriptJson,
      aiSummary = excluded.aiSummary,
      lastMessageTimestamp = excluded.lastMessageTimestamp,
      updatedAt = excluded.updatedAt
  `;
}

export async function getThreadSummary(
  snowflake: string,
): Promise<ThreadSummaryRow | undefined> {
  const rows = await db`
    SELECT *
    FROM thread_summaries
    WHERE snowflake = ${snowflake}
    LIMIT 1
  `;
  return rows[0];
}

export async function listThreadSummaries(): Promise<ThreadSummaryRow[]> {
  const rows = await db`
    SELECT *
    FROM thread_summaries
    ORDER BY lastMessageTimestamp DESC
  `;
  return rows as ThreadSummaryRow[];
}
