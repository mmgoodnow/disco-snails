import ms from "ms";
import { processThreads } from "./discordProcessor";
import { startServer } from "./server";

const port = Number(process.env.PORT ?? 3000);
startServer(port);

setInterval(async () => {
  try {
    await processThreads();
  } catch (err) {
    console.error("Failed to process Discord threads", err);
  }
}, ms("1 day"));
