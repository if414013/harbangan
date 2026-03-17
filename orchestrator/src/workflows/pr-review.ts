import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import { GitHubTools } from "../mcp/github-tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";
import { createAskUserMcpServer } from "../slack/ask-user-tool.js";
import { prReviewStartedBlocks, reviewSummaryBlocks, taskCreatedBlocks } from "../slack/blocks.js";
import type { ITaskStore } from "../store/types.js";
import type { TaskQueue } from "../queue/task-queue.js";

const log = createChildLogger("workflow:pr-review");

/**
 * PR Review workflow: multi-dimensional code review with GitHub comments.
 *
 * When Slack is configured:
 * - Posts review summary to Slack
 * - Auto-creates fix tasks for confident issues
 * - Asks user in Slack for uncertain findings before creating fix tasks
 */
export async function executePrReviewWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
  ghToken: string,
  store?: ITaskStore,
  queue?: TaskQueue,
): Promise<WorkflowResult> {
  const prNumber = ctx.task.input.pr_number;
  if (!prNumber) {
    return { success: false, output: "PR number is required", cost_usd: 0 };
  }

  // Skip auto-fix for PRs created from auto_fix tasks (loop prevention)
  const isAutoFixPr = ctx.task.input.auto_fix === true;

  log.info({ taskId: ctx.task.id, prNumber, isAutoFixPr }, "Starting PR review workflow");

  const github = new GitHubTools(ghToken);

  // Fetch PR info and diff
  let prInfo: string;
  let diff: string;
  try {
    prInfo = github.getPrInfo(prNumber);
    diff = github.getPrDiff(prNumber);
  } catch (err) {
    const msg = `Failed to fetch PR #${prNumber}: ${err instanceof Error ? err.message : String(err)}`;
    log.error({ prNumber, error: msg }, "PR fetch failed");
    return { success: false, output: msg, cost_usd: 0 };
  }

  // Set up Slack thread for review notifications
  const hasSlack = ctx.slackService && ctx.interactionBridge;
  let slackChannelId = ctx.task.input.slack_channel_id;
  let slackThreadTs: string | undefined;
  let mcpServers: Record<string, ReturnType<typeof createAskUserMcpServer>> | undefined;

  if (hasSlack && ctx.slackService && slackChannelId) {
    try {
      // Extract PR title from prInfo (first line typically)
      const prTitle = prInfo.split("\n")[0] ?? `PR #${prNumber}`;
      slackThreadTs = await ctx.slackService.postMessage(
        slackChannelId,
        `📝 Reviewing PR #${prNumber}`,
        prReviewStartedBlocks(prNumber, prTitle),
      );

      // Register thread for interactive follow-up
      if (ctx.interactionBridge) {
        ctx.interactionBridge.registerThread(ctx.task.id, slackChannelId, slackThreadTs);

        // Create MCP server with ask_user tool for uncertain findings
        const mcpServer = createAskUserMcpServer(ctx.task.id, ctx.slackService);
        mcpServers = { "slack-interaction": mcpServer };
      }
    } catch (err) {
      log.warn({ error: String(err) }, "Failed to set up Slack for PR review");
    }
  }

  // Use a review-capable agent
  const agents = registry.getForWorkflow("pr-review");
  const agent = agents[0];
  if (!agent) {
    return { success: false, output: "No review-capable agents found", cost_usd: 0 };
  }

  const tools = buildToolSet("pr-review");

  // Retrieve relevant knowledge context
  let kbContext = "";
  if (ctx.knowledgeRetriever) {
    const kb = await ctx.knowledgeRetriever.getContext(
      `PR #${prNumber} review ${prInfo.slice(0, 200)}`,
      ["decision", "learning", "incident"],
    );
    kbContext = kb.formatted;
  }

  const interactiveInstructions = mcpServers && !isAutoFixPr
    ? `\n\n## Interactive Mode
You have access to the \`ask_user\` tool. Use it when:
- You found issues but are uncertain whether they warrant a fix task
- You want user input on severity or priority of findings
- You need clarification on project conventions

For confident findings that clearly need fixes, note them as "CONFIDENT_FIX: <description>" in your output.
For uncertain findings, use ask_user to check with the user before recommending a fix task.`
    : "";

  const prompt = `You are reviewing PR #${prNumber} on the Harbangan repository.

## PR Metadata
${prInfo}
${kbContext ? `\n${kbContext}\n` : ""}

## Diff
\`\`\`diff
${diff.slice(0, 50000)}
\`\`\`

## Review Dimensions
For each relevant dimension, provide feedback:

1. **Correctness**: Logic errors, edge cases, off-by-one errors
2. **Security**: Input validation, auth checks, injection risks, secret exposure
3. **Performance**: N+1 queries, unnecessary allocations, missing caching
4. **Architecture**: Module boundaries, coupling, DRY violations
5. **Testing**: Missing tests, inadequate coverage, test quality
6. **Style**: Naming, formatting, import order, idiomatic patterns

## Instructions
- Read the diff carefully and explore referenced files in the codebase for context
- Focus on substantive issues, not nitpicks
- For each issue found, format as:
  **[DIMENSION] File:Line** - Description of issue and suggested fix
- At the end, provide an overall assessment: APPROVE, REQUEST_CHANGES, or COMMENT
- Include a summary paragraph

## Harbangan Conventions
- Backend: Rust/Axum, thiserror+anyhow errors, tracing logging, no .unwrap()
- Frontend: React 19, TypeScript strict, named exports, CRT aesthetic
- Tests: test_<func>_<scenario> naming, #[tokio::test] for async
- Serde: snake_case, skip_serializing_if for Option fields${interactiveInstructions}`;

  const result = await executeAgent(agent, prompt, tools, ctx, {
    workdir: ctx.repoPath,
    mcpServers,
  });

  // Clean up Slack interaction
  if (ctx.interactionBridge) {
    ctx.interactionBridge.cleanup(ctx.task.id);
  }

  // Post review to GitHub if successful
  if (result.success && result.output) {
    try {
      const event = result.output.includes("REQUEST_CHANGES")
        ? "REQUEST_CHANGES" as const
        : result.output.includes("APPROVE")
          ? "APPROVE" as const
          : "COMMENT" as const;

      github.submitPrReview(prNumber, event, result.output.slice(0, 65000));
      log.info({ prNumber, event }, "PR review posted to GitHub");

      // Post summary to Slack
      if (ctx.slackService && slackChannelId) {
        const summaryLines = result.output.split("\n").slice(0, 10).join("\n");
        ctx.slackService.postMessage(
          slackChannelId,
          `${event === "APPROVE" ? "✅" : "🔴"} PR #${prNumber} — ${event}`,
          reviewSummaryBlocks(prNumber, event, summaryLines),
          slackThreadTs,
        ).catch(() => {});
      }

      // Auto-create fix task for confident issues (skip for auto_fix PRs to prevent loops)
      if (!isAutoFixPr && event === "REQUEST_CHANGES" && store && queue) {
        const hasConfidentFixes = result.output.includes("CONFIDENT_FIX:");
        if (hasConfidentFixes) {
          try {
            const fixTask = store.createTask(
              "implement",
              {
                pr_number: prNumber,
                description: `Fix issues in PR #${prNumber}`,
                auto_fix: true,
                slack_channel_id: slackChannelId,
              },
              "normal",
              ctx.task.budget_usd,
            );
            store.updateTaskStatus(fixTask.id, "queued");
            queue.enqueue({ ...fixTask, status: "queued" });

            log.info({ prNumber, fixTaskId: fixTask.id }, "Auto-created fix task");

            if (ctx.slackService && slackChannelId) {
              ctx.slackService.postMessage(
                slackChannelId,
                `🔧 Fix task ${fixTask.id.slice(0, 8)} created for PR #${prNumber}`,
                taskCreatedBlocks(fixTask.id, "implement", `Fix issues in PR #${prNumber}`),
                slackThreadTs,
              ).catch(() => {});
            }
          } catch (err) {
            log.error({ error: String(err) }, "Failed to auto-create fix task");
          }
        }
      }
    } catch (err) {
      log.error(
        { prNumber, error: String(err) },
        "Failed to post PR review",
      );
    }
  }

  return result;
}
