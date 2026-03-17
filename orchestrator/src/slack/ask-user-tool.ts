import { z } from "zod";
import { createSdkMcpServer, tool } from "@anthropic-ai/claude-agent-sdk";
import type { SlackService } from "./service.js";

/**
 * Creates an in-process MCP server with the `ask_user` tool.
 *
 * When an agent calls this tool, it posts the question to the task's
 * Slack thread and blocks until the user replies or timeout.
 */
export function createAskUserMcpServer(
  taskId: string,
  slackService: SlackService,
) {
  const askUserTool = tool(
    "ask_user",
    "Ask the user a question in the Slack thread and wait for their reply. Use this when you need clarification, want to present options, or need approval to proceed.",
    { question: z.string().describe("The question to ask the user") },
    async (args) => {
      try {
        const reply = await slackService.askUserInThread(taskId, args.question);
        return {
          content: [{ type: "text" as const, text: reply }],
        };
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        return {
          content: [{ type: "text" as const, text: `Error: ${msg}` }],
          isError: true,
        };
      }
    },
  );

  return createSdkMcpServer({
    name: "slack-interaction",
    version: "1.0.0",
    tools: [askUserTool],
  });
}
