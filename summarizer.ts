import OpenAI from "openai";

const openai = new OpenAI();
const OPENAI_MODEL = process.env.OPENAI_MODEL ?? "gpt-5-mini";

export type TranscriptMessage = {
  user: string;
  content: string;
};

function indented(content: string) {
  return content
    .split("\n")
    .map((line) => `\t${line}`)
    .join("\n");
}

export async function summarizeThread(
  title: string,
  messages: TranscriptMessage[],
) {
  const transcript = messages
    .map((message) => `${message.user}:\n${indented(message.content)}\n`)
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
- What improvements could we make to the docs or the app that would help? (0 to 1 improvements only)

Return 3-4 CONCISE notes formatted as HTML with <h4> and <p> tags.
`;

  const res = await openai.chat.completions.create({
    model: OPENAI_MODEL,
    messages: [{ role: "user", content: prompt }],
  });

  return res.choices[0]?.message?.content?.trim() ?? "(no summary)";
}
