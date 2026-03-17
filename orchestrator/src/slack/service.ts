import pkg from "@slack/bolt";
const { App } = pkg;
import { createChildLogger } from "../util/logger.js";
import type { InteractionBridge } from "./interaction-bridge.js";
import type { SlackServiceConfig } from "./types.js";
import type { SlackBlock } from "./blocks.js";
import { questionBlocks } from "./blocks.js";

const log = createChildLogger("slack-service");

/**
 * Wraps @slack/bolt App with Socket Mode.
 * Handles /harbangan slash commands and thread reply routing.
 */
export class SlackService {
  private app: InstanceType<typeof App>;
  private bridge: InteractionBridge;
  private botUserId: string | undefined;

  /** Callback invoked when a /harbangan command is received. */
  onCommand: ((command: string, args: string, channelId: string, userId: string) => Promise<{ text: string; threadTs: string } | null>) | null = null;

  constructor(config: SlackServiceConfig, bridge: InteractionBridge) {
    this.bridge = bridge;

    this.app = new App({
      token: config.botToken,
      appToken: config.appToken,
      socketMode: true,
      // Disable built-in HTTP receiver since we use Socket Mode
    });

    this.setupCommandHandler();
    this.setupMessageHandler();
  }

  private setupCommandHandler(): void {
    this.app.command("/harbangan", async ({ command, ack }) => {
      await ack();

      const text = command.text.trim();
      const parts = text.split(/\s+/);
      const subcommand = parts[0]?.toLowerCase() ?? "";
      const args = parts.slice(1).join(" ");

      log.info(
        { user: command.user_id, command: subcommand, args, channel: command.channel_id },
        "Slash command received",
      );

      if (this.onCommand) {
        try {
          await this.onCommand(subcommand, args, command.channel_id, command.user_id);
        } catch (err) {
          log.error({ error: String(err) }, "Command handler failed");
        }
      }
    });
  }

  private setupMessageHandler(): void {
    // Listen for messages in threads where we have active interactions
    this.app.message(async ({ message }) => {
      // Only handle thread replies, not top-level messages
      // Cast through unknown for Slack bolt message type narrowing
      const msg = message as unknown as {
        thread_ts?: string;
        text?: string;
        bot_id?: string;
        subtype?: string;
        user?: string;
      };
      if (!msg.thread_ts || typeof msg.thread_ts !== "string") return;
      if (typeof msg.text !== "string") return;

      // Ignore bot's own messages
      if (msg.bot_id || msg.subtype === "bot_message") return;
      if (this.botUserId && msg.user === this.botUserId) return;

      const threadTs = msg.thread_ts;
      const text = msg.text;

      const resolved = this.bridge.handleReply(threadTs, text);
      if (resolved) {
        log.info({ threadTs, user: msg.user }, "Thread reply resolved pending interaction");
      }
    });
  }

  /**
   * Post a message to a channel. Returns the message timestamp.
   */
  async postMessage(
    channelId: string,
    text: string,
    blocks?: SlackBlock[],
    threadTs?: string,
  ): Promise<string> {
    const result = await this.app.client.chat.postMessage({
      channel: channelId,
      text,
      blocks: blocks as never[],
      ...(threadTs ? { thread_ts: threadTs } : {}),
    });

    return result.ts as string;
  }

  /**
   * Post a question to a task's thread and register it with the bridge.
   * Returns a Promise that resolves with the user's reply.
   */
  async askUserInThread(taskId: string, question: string): Promise<string> {
    const binding = this.bridge.getBinding(taskId);
    if (!binding) {
      throw new Error(`No thread binding for task ${taskId}`);
    }

    // Post the question to the thread
    await this.postMessage(
      binding.channelId,
      question,
      questionBlocks(question),
      binding.threadTs,
    );

    // Block until user replies or timeout
    return this.bridge.askUser(taskId, question);
  }

  /**
   * Start the Slack Socket Mode connection.
   */
  async start(): Promise<void> {
    await this.app.start();

    // Fetch bot user ID for filtering own messages
    try {
      const auth = await this.app.client.auth.test();
      this.botUserId = auth.user_id as string;
      log.info({ botUserId: this.botUserId }, "Slack Socket Mode connected");
    } catch (err) {
      log.warn({ error: String(err) }, "Could not fetch bot user ID");
    }
  }

  /**
   * Stop the Slack connection.
   */
  async stop(): Promise<void> {
    await this.app.stop();
    log.info("Slack service stopped");
  }
}
