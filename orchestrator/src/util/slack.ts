import { createChildLogger } from "./logger.js";

const log = createChildLogger("slack");

/**
 * Slack notification helper.
 * Posts messages to Slack channels when tasks complete.
 */
export class SlackNotifier {
  private botToken: string | undefined;

  constructor(botToken?: string) {
    this.botToken = botToken;
  }

  get isConfigured(): boolean {
    return !!this.botToken;
  }

  async notify(
    channelId: string,
    message: string,
  ): Promise<void> {
    if (!this.botToken) {
      log.debug("Slack not configured, skipping notification");
      return;
    }

    try {
      const response = await fetch("https://slack.com/api/chat.postMessage", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.botToken}`,
        },
        body: JSON.stringify({
          channel: channelId,
          text: message,
          mrkdwn: true,
        }),
      });

      const data = (await response.json()) as { ok: boolean; error?: string };
      if (!data.ok) {
        log.error({ error: data.error, channel: channelId }, "Slack notification failed");
      } else {
        log.info({ channel: channelId }, "Slack notification sent");
      }
    } catch (err) {
      log.error({ error: String(err) }, "Slack API call failed");
    }
  }

  /**
   * Format a task completion message for Slack.
   */
  formatTaskComplete(
    taskId: string,
    type: string,
    success: boolean,
    output: string,
    prUrl?: string | null,
    costUsd?: number,
  ): string {
    const status = success ? "✅ Completed" : "❌ Failed";
    const truncatedOutput = output.length > 1000
      ? output.slice(0, 997) + "..."
      : output;

    let msg = `*${status}* — \`${type}\` task \`${taskId.slice(0, 8)}\`\n`;
    if (prUrl) {
      msg += `📎 PR: ${prUrl}\n`;
    }
    if (costUsd !== undefined) {
      msg += `💰 Cost: $${costUsd.toFixed(4)}\n`;
    }
    msg += `\`\`\`\n${truncatedOutput}\n\`\`\``;
    return msg;
  }
}
