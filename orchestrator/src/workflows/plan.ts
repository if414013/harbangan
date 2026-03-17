import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";
import { createAskUserMcpServer } from "../slack/ask-user-tool.js";
import { planStartedBlocks, taskCreatedBlocks } from "../slack/blocks.js";
import type { ITaskStore } from "../store/types.js";
import type { TaskQueue } from "../queue/task-queue.js";

const log = createChildLogger("workflow:plan");

/**
 * Plan workflow: analyzes scope and produces a structured implementation plan.
 *
 * When Slack is configured, the agent can interactively ask the user questions
 * via the ask_user MCP tool. On plan approval, auto-creates an implement task.
 */
export async function executePlanWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
  store?: ITaskStore,
  queue?: TaskQueue,
): Promise<WorkflowResult> {
  const description =
    ctx.task.input.description ??
    `Issue #${ctx.task.input.issue_number}`;

  log.info({ taskId: ctx.task.id, description }, "Starting plan workflow");

  // Use the first available plan-capable agent
  const agents = registry.getForWorkflow("plan");
  const agent = agents[0];
  if (!agent) {
    return { success: false, output: "No plan-capable agents found", cost_usd: 0 };
  }

  const tools = buildToolSet("plan");

  // Set up interactive Slack thread if available
  const hasSlack = ctx.slackService && ctx.interactionBridge;
  let mcpServers: Record<string, ReturnType<typeof createAskUserMcpServer>> | undefined;

  if (hasSlack && ctx.slackService && ctx.interactionBridge) {
    // Post the "planning started" message and register the thread
    const channelId = ctx.task.input.slack_channel_id;
    if (channelId) {
      try {
        const threadTs = await ctx.slackService.postMessage(
          channelId,
          `🔍 Planning: ${description}`,
          planStartedBlocks(ctx.task.id, description),
        );
        ctx.interactionBridge.registerThread(ctx.task.id, channelId, threadTs);

        // Create MCP server with ask_user tool
        const mcpServer = createAskUserMcpServer(ctx.task.id, ctx.slackService);
        mcpServers = { "slack-interaction": mcpServer };

        log.info({ taskId: ctx.task.id, channelId, threadTs }, "Interactive planning enabled");
      } catch (err) {
        log.warn({ error: String(err) }, "Failed to set up interactive Slack thread, continuing without");
      }
    }
  }

  // Retrieve relevant knowledge context
  let kbContext = "";
  if (ctx.knowledgeRetriever) {
    const kb = await ctx.knowledgeRetriever.getContext(description, ["decision", "learning"]);
    kbContext = kb.formatted;
  }

  const interactiveInstructions = mcpServers
    ? `\n\n## Interactive Mode
You have access to the \`ask_user\` tool. Use it to:
- Ask clarifying questions about requirements or scope
- Present design options and let the user choose
- At the END of your plan, ask "Here's the plan. Should I proceed with implementation? (yes/no)" to get approval

Do NOT proceed without user confirmation at the end.`
    : "";

  const prompt = `You are analyzing the Harbangan codebase to create an implementation plan.

## Task
${description}
${kbContext ? `\n${kbContext}\n` : ""}

## Instructions
1. Explore the codebase to understand which modules, files, and patterns are relevant
2. Identify all affected services (backend, frontend, database, infrastructure)
3. Assess complexity and risks
4. Produce a structured implementation plan

## Plan Format
Your plan MUST include:

### Scope Analysis
- What services are affected
- What files need changes
- Dependencies between changes

### Consultation Summary
For each affected area, describe:
- Current patterns and code structure
- Specific files to modify
- Risks and gotchas

### Task Decomposition
Organize into waves for parallel execution:
- Wave 1 (foundations): Backend types, DB migrations, core logic
- Wave 2 (consumers): Frontend pages, API integration
- Wave 3 (verification): Unit tests, E2E tests

### Agent Assignment
Map each task to a specific agent:
- rust-backend-engineer: Backend Rust code
- react-frontend-engineer: Frontend React code
- database-engineer: Schema/migration changes
- devops-engineer: Docker/infrastructure
- backend-qa: Backend tests
- frontend-qa: E2E tests

### File Ownership
Assign each file to exactly one agent. No overlaps.

### Team Preset
Recommend: backend-feature, frontend-feature, fullstack, or infra

### Estimated Budget
Rough cost estimate for the full implementation.

Be thorough and specific. Reference actual file paths and code patterns from the codebase.${interactiveInstructions}`;

  const result = await executeAgent(agent, prompt, tools, ctx, {
    workdir: ctx.repoPath,
    mcpServers,
  });

  // Clean up Slack interaction
  if (ctx.interactionBridge) {
    ctx.interactionBridge.cleanup(ctx.task.id);
  }

  // Auto-create implement task if plan succeeded and user approved (via ask_user "yes")
  if (result.success && store && queue) {
    const output = result.output.toLowerCase();
    const approved = output.includes("proceed with implementation") ||
      output.includes("user approved") ||
      output.includes("user confirmed");

    if (approved || !mcpServers) {
      // In non-interactive mode, always auto-create; in interactive, only if approved
      try {
        const implTask = store.createTask(
          "implement",
          {
            plan_task_id: ctx.task.id,
            description: `Implement: ${description}`,
            slack_channel_id: ctx.task.input.slack_channel_id,
          },
          "normal",
          ctx.task.budget_usd,
        );
        store.updateTaskStatus(implTask.id, "queued");
        queue.enqueue({ ...implTask, status: "queued" });

        log.info({ taskId: ctx.task.id, implTaskId: implTask.id }, "Auto-created implement task");

        // Notify in Slack thread
        if (ctx.slackService && ctx.task.input.slack_channel_id) {
          const binding = ctx.interactionBridge?.getBinding(ctx.task.id);
          ctx.slackService.postMessage(
            ctx.task.input.slack_channel_id,
            `✅ Implementation task ${implTask.id.slice(0, 8)} created`,
            taskCreatedBlocks(implTask.id, "implement", description),
            binding?.threadTs,
          ).catch(() => {});
        }
      } catch (err) {
        log.error({ error: String(err) }, "Failed to auto-create implement task");
      }
    }
  }

  return result;
}
