import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";

const log = createChildLogger("workflow:kanban");

/**
 * Kanban Management workflow: GitHub issue CRUD and board operations.
 *
 * Commands: create-issue, update-status, board-summary, close-issues
 */
export async function executeKanbanWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
): Promise<WorkflowResult> {
  const command = ctx.task.input.command ?? ctx.task.input.description ?? "board-summary";

  log.info({ taskId: ctx.task.id, command }, "Starting kanban workflow");

  const agents = registry.getForWorkflow("kanban");
  const agent = agents[0];
  if (!agent) {
    return { success: false, output: "No kanban agents found", cost_usd: 0 };
  }

  const tools = buildToolSet("kanban");

  const prompt = `You are managing the Harbangan project board on GitHub.

## Command
${command}

## Repository
Owner: if414013
Repo: harbangan

## Instructions
Use the gh CLI to execute the requested board management operation.

Available operations:
- board-summary: List all open issues with their status, priority, and assignee
- create-issue: Create a new issue with proper labels and board fields
- update-status: Update issue status on the project board
- close-issues: Close specified issues
- list-blocked: Show issues with status:blocked label

## Board Fields
- Status: Backlog, Ready, In progress, In review, Done
- Priority: P0 (critical), P1 (important), P2 (nice-to-have)
- Size: XS, S, M, L, XL

## Issue Format
- Title: [service]: description
- Labels: service label, priority, type (feature/bug/refactor)
- Always include priority and size

Execute the command and report the results clearly.`;

  return executeAgent(agent, prompt, tools, ctx, ctx.repoPath);
}
