/**
 * Slack Block Kit message builders for interactive workflows.
 */

export interface SlackBlock {
  type: string;
  text?: { type: string; text: string; emoji?: boolean };
  elements?: Array<{ type: string; text: string }>;
  fields?: Array<{ type: string; text: string }>;
}

/**
 * Build the initial "planning started" message blocks.
 */
export function planStartedBlocks(taskId: string, description: string): SlackBlock[] {
  return [
    {
      type: "header",
      text: { type: "plain_text", text: "🔍 Planning Started", emoji: true },
    },
    {
      type: "section",
      text: {
        type: "mrkdwn",
        text: `*Task:* \`${taskId.slice(0, 8)}\`\n*Description:* ${description}`,
      },
    },
    {
      type: "context",
      elements: [
        {
          type: "mrkdwn",
          text: "The agent will ask questions in this thread. Reply here to answer.",
        },
      ],
    },
  ];
}

/**
 * Build a question message from the agent to the user.
 */
export function questionBlocks(question: string): SlackBlock[] {
  return [
    {
      type: "section",
      text: { type: "mrkdwn", text: `❓ *Agent question:*\n${question}` },
    },
    {
      type: "context",
      elements: [
        { type: "mrkdwn", text: "Reply in this thread to answer." },
      ],
    },
  ];
}

/**
 * Build a "PR review started" message.
 */
export function prReviewStartedBlocks(prNumber: number, prTitle: string): SlackBlock[] {
  return [
    {
      type: "header",
      text: { type: "plain_text", text: `📝 Reviewing PR #${prNumber}`, emoji: true },
    },
    {
      type: "section",
      text: { type: "mrkdwn", text: `*${prTitle}*` },
    },
  ];
}

/**
 * Build a review summary message.
 */
export function reviewSummaryBlocks(
  prNumber: number,
  verdict: "APPROVE" | "REQUEST_CHANGES" | "COMMENT",
  summary: string,
): SlackBlock[] {
  const emoji = verdict === "APPROVE" ? "✅" : verdict === "REQUEST_CHANGES" ? "🔴" : "💬";
  return [
    {
      type: "section",
      text: {
        type: "mrkdwn",
        text: `${emoji} *PR #${prNumber} — ${verdict}*\n\n${summary.slice(0, 2900)}`,
      },
    },
  ];
}

/**
 * Build a task-created notification.
 */
export function taskCreatedBlocks(taskId: string, taskType: string, description: string): SlackBlock[] {
  const emoji = taskType === "implement" ? "🔧" : "📋";
  return [
    {
      type: "section",
      text: {
        type: "mrkdwn",
        text: `${emoji} *${taskType} task created:* \`${taskId.slice(0, 8)}\`\n${description}`,
      },
    },
  ];
}

/**
 * Build a simple text message (fallback for plain notifications).
 */
export function textBlock(text: string): SlackBlock[] {
  return [{ type: "section", text: { type: "mrkdwn", text } }];
}
