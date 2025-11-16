import {
  type ThreadChannel,
  Client as DiscordClient,
  GatewayIntentBits,
  ForumChannel,
  type Message,
} from "discord.js";
import { summarizeThread, type TranscriptMessage } from "./summarizer";
import {
  getThreadSummary,
  upsertThreadSummary,
  type ThreadSummaryRow,
} from "./db";

const LOOKBACK = Number(process.env.LOOKBACK ?? 2);
const VERBOSE_LOG = process.env.DISCORD_VERBOSE_LOGS === "true";

function verboseLog(...args: Parameters<typeof console.log>) {
  if (VERBOSE_LOG) {
    console.log(...args);
  }
}

function verboseGroup(label: string) {
  if (VERBOSE_LOG) {
    console.group(label);
  }
}

function verboseGroupEnd() {
  if (VERBOSE_LOG) {
    console.groupEnd();
  }
}

const FORUM_CHANNEL_ID = process.env.DISCORD_FORUM_CHANNEL_ID;

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

type FetchedTranscript = {
  transcript: TranscriptMessage[];
  lastMessageTimestamp: number;
};

async function fetchAllMessages(
  thread: ThreadChannel,
  existing?: ThreadSummaryRow,
): Promise<FetchedTranscript | undefined> {
  const newestBatch = await thread.messages.fetch({ limit: 1 });
  const newestMessage = newestBatch.first();
  if (!newestMessage) return undefined;

  if (
    existing &&
    existing.lastMessageTimestamp === newestMessage.createdTimestamp
  ) {
    return undefined;
  }

  const all: Message[] = [...newestBatch.values()];
  let before = all[all.length - 1]?.id;

  while (true) {
    const batch = await thread.messages.fetch({
      limit: 100,
      ...(before ? { before } : {}),
    });

    if (batch.size === 0) break;
    const messages = [...batch.values()];
    all.push(...messages);
    before = messages[messages.length - 1].id;
  }

  const transcript = all.reverse().map((message) => ({
    user: message.author.globalName ?? message.author.username,
    content: message.content,
  }));

  return {
    transcript,
    lastMessageTimestamp: newestMessage.createdTimestamp,
  };
}

type ThreadTranscript = {
  threadId: string;
  threadName: string;
  transcript: TranscriptMessage[];
  lastMessageTimestamp: number;
  hasChanges: boolean;
};

async function* streamThreadTranscripts(
  forumChannel: ForumChannel,
  lookback: number,
): AsyncGenerator<ThreadTranscript> {
  const archived = await forumChannel.threads.fetchArchived({
    type: "public",
    limit: lookback,
  });

  verboseLog(`Fetched ${archived.threads.size} archived threads`);

  for (const [, thread] of archived.threads) {
    const existing = await getThreadSummary(thread.id);
    const fetched = await fetchAllMessages(thread, existing);

    if (!fetched) {
      yield {
        threadId: thread.id,
        threadName: thread.name,
        lastMessageTimestamp: existing?.lastMessageTimestamp ?? 0,
        transcript: [],
        hasChanges: false,
      };
      continue;
    }

    yield {
      threadId: thread.id,
      threadName: thread.name,
      lastMessageTimestamp: fetched.lastMessageTimestamp,
      transcript: fetched.transcript,
      hasChanges: true,
    };
  }
}

export async function processThreads() {
  if (!FORUM_CHANNEL_ID) {
    throw new Error("DISCORD_FORUM_CHANNEL_ID is not set");
  }
  const discord = await getDiscordClient();

  const forumChannel = (await discord.channels.fetch(
    FORUM_CHANNEL_ID,
  )) as ForumChannel;

  let processedCount = 0;
  let skippedCount = 0;
  for await (const threadData of streamThreadTranscripts(
    forumChannel,
    LOOKBACK,
  )) {
    verboseGroup(`Thread "${threadData.threadName}"`);
    verboseLog("Starting");

    if (!threadData.hasChanges) {
      verboseLog("Skipping (no new messages)");
      verboseGroupEnd();
      skippedCount += 1;
      continue;
    }

    verboseLog(`Fetched ${threadData.transcript.length} messages`);
    const summaryLabel = VERBOSE_LOG
      ? "Summarizing"
      : `Summarizing "${threadData.threadName}"`;
    console.log(summaryLabel);
    const aiSummary = await summarizeThread(
      threadData.threadName,
      threadData.transcript,
    );

    await upsertThreadSummary(
      threadData.threadId,
      threadData.threadName,
      JSON.stringify(threadData.transcript),
      aiSummary,
      threadData.lastMessageTimestamp,
    );
    verboseGroupEnd();
    processedCount += 1;
  }

  if (processedCount === 0) {
    console.log(`${skippedCount} threads were up-to-date; no new summaries.`);
  } else {
    console.log(
      `Processed ${processedCount} threads (${skippedCount} skipped without changes).`,
    );
  }
}
