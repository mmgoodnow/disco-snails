import OpenAI from "openai";
import {
  type ThreadChannel,
  Client as DiscordClient,
  GatewayIntentBits,
  ForumChannel,
} from "discord.js";
import { SQL } from "bun";
import ms from "ms";
const openai = new OpenAI();
const db = new SQL("sqlite://snails.db");

interface ThreadSummaryRow {
  snowflake: string;
  name: string;
  aiSummary: string;
  lastMessageTimestamp: number;
  updatedAt: number;
  transcriptJson: string;
}

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

async function upsertThreadSummary(
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

async function getThreadSummary(
  snowflake: string,
): ThreadSummaryRow | undefined {
  const rows = await db`
    SELECT *
    FROM thread_summaries
    WHERE snowflake = ${snowflake}
    LIMIT 1
  `;
  return rows[0];
}

async function getDiscordClient() {
  const discord = new DiscordClient({
    intents: [
      GatewayIntentBits.Guilds,
      GatewayIntentBits.GuildMessages,
      GatewayIntentBits.MessageContent,
    ],
  });

  await new Promise((resolve) => {
    discord.login(process.env.DISCORD_BOT_TOKEN);
    discord.once("clientReady", () => {
      resolve();
    });
  });
  return discord;
}

async function fetchAllMessages(thread: ThreadChannel) {
  const all: any[] = [];
  let before: string | undefined = undefined;
  while (true) {
    const batch = await thread.messages.fetch({
      limit: 100,
      ...(before ? { before } : {}),
    });

    if (batch.size === 0) break;
    const msgs = [...batch.values()];
    all.push(...msgs);
    before = msgs[msgs.length - 1].id;
    if (all.length >= 500) break;
  }
  return all.reverse();
}

function indented(content) {
  return content
    .split("\n")
    .map((l) => `\t${l}`)
    .join("\n");
}

async function summarize(title, messages) {
  console.log("Summarizing", title);
  const transcript = messages
    .map((m) => `${m.user}:\n${indented(m.content)}\n`)
    .join("");

  const prompt = `
You are summarizing a Discord support thread for cross-seed, a BitTorrent cross-seeding automation tool: https://cross-seed.org.

Thread title: "${title}"

Conversation:
${transcript}

I am the primary developer and don't have time to read these threads. Summarize this thread:
- What was the user's problem?
- What troubleshooting was done?
- What was the final resolution (or current status)?
- What improvements could we make to the docs or the app that would resolve or clarify this?

Return 3-6 bullet points.
`;

  const res = await openai.chat.completions.create({
    model: "gpt-5-nano",
    messages: [{ role: "user", content: prompt }],
  });

  return res.choices[0]?.message?.content?.trim() ?? "(no summary)";
}

async function processThreads() {
  const discord = await getDiscordClient();

  const forumChannel = (await discord.channels.fetch(
    "1084972377529667584",
  )) as ForumChannel;

  const archived = await forumChannel.threads.fetchArchived({
    type: "public",
    limit: 2,
  });

  for (const [snowflake, thread] of archived.threads) {
    const messageObjects = await fetchAllMessages(thread);
    const lastMessage = messageObjects.at(-1);
    const existing = await getThreadSummary(thread.id);

    if (
      !existing ||
      existing.lastMessageTimestamp !== lastMessage.createdTimestamp
    ) {
      const transcript = messageObjects.map((m) => ({
        user: m.author.username,
        content: m.content,
      }));
      const aiSummary = await summarize(thread.name, transcript);

      await upsertThreadSummary(
        thread.id,
        thread.name,
        JSON.stringify(transcript),
        aiSummary,
        lastMessage.createdTimestamp,
      );
    }
  }
}

setInterval(processThreads, ms("1 day"));
