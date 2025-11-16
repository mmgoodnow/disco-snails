import {
  type ThreadChannel,
  Client as DiscordClient,
  GatewayIntentBits,
  ForumChannel,
} from "discord.js";
import { summarizeThread, type TranscriptMessage } from "./summarizer";
import { getThreadSummary, upsertThreadSummary } from "./db";

const LOOKBACK = Number(process.env.LOOKBACK ?? 2);

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
    discord.once("clientReady", () => resolve(undefined));
  });

  return discord;
}

async function fetchAllMessages(thread: ThreadChannel) {
  const all: any[] = [];
  let before: string | undefined;

  while (true) {
    const batch = await thread.messages.fetch({
      limit: 100,
      ...(before ? { before } : {}),
    });

    if (batch.size === 0) break;
    const messages = [...batch.values()];
    all.push(...messages);
    before = messages[messages.length - 1].id;
    if (all.length >= 500) break;
  }

  return all.reverse();
}

export async function processThreads() {
  const discord = await getDiscordClient();

  const forumChannel = (await discord.channels.fetch(
    "1084972377529667584",
  )) as ForumChannel;

  const archived = await forumChannel.threads.fetchArchived({
    type: "public",
    limit: LOOKBACK,
  });

  for (const [, thread] of archived.threads) {
    const messages = await fetchAllMessages(thread);
    const lastMessage = messages.at(-1);
    if (!lastMessage) continue;

    const existing = await getThreadSummary(thread.id);
    if (
      !existing ||
      existing.lastMessageTimestamp !== lastMessage.createdTimestamp
    ) {
      const transcript: TranscriptMessage[] = messages.map((message) => ({
        user: message.author.username,
        content: message.content,
      }));

      const aiSummary = await summarizeThread(thread.name, transcript);

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
