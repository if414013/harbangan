import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import { GitHubTools } from "../mcp/github-tools.js";
import { HarbanganTools } from "../mcp/harbangan-tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";

const log = createChildLogger("workflow:implement");

/**
 * Feature Implementation workflow: full lifecycle with worktrees.
 *
 * Steps:
 * 1. Read issue or accept description
 * 2. Create worktree + feature branch
 * 3. Execute implementation agent
 * 4. Run quality gates
 * 5. Commit, push, create PR
 * 6. Clean up worktree
 */
export async function executeImplementWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
  ghToken: string,
): Promise<WorkflowResult> {
  const issueNumber = ctx.task.input.issue_number;
  const description = ctx.task.input.description;
  const planTaskId = ctx.task.input.plan_task_id;

  if (!issueNumber && !description && !planTaskId) {
    return {
      success: false,
      output: "Either issue_number, description, or plan_task_id is required",
      cost_usd: 0,
    };
  }

  log.info(
    { taskId: ctx.task.id, issueNumber, description: description?.slice(0, 100) },
    "Starting implement workflow",
  );

  const github = new GitHubTools(ghToken);
  const harbangan = new HarbanganTools();

  // Get task description
  let taskDescription = description ?? "";
  if (issueNumber) {
    try {
      taskDescription = github.getIssueDetails(issueNumber);
    } catch (err) {
      log.warn({ issueNumber, error: String(err) }, "Failed to fetch issue");
    }
  }
  if (planTaskId) {
    const planTask = ctx.store.getTask(planTaskId);
    if (planTask?.output) {
      taskDescription = planTask.output;
    }
  }

  // Create worktree
  const branchName = issueNumber
    ? `feat/issue-${issueNumber}`
    : `feat/task-${ctx.task.id.slice(0, 8)}`;

  const branchCheck = ctx.guardrails.validateBranchName(branchName);
  if (!branchCheck.allowed) {
    return { success: false, output: branchCheck.reason!, cost_usd: 0 };
  }

  let worktreePath: string;
  try {
    const worktree = ctx.worktrees.create(ctx.task.id, branchName);
    worktreePath = worktree.path;
    ctx.store.updateTaskStatus(ctx.task.id, "running", { branch: branchName });
  } catch (err) {
    return {
      success: false,
      output: `Failed to create worktree: ${err instanceof Error ? err.message : String(err)}`,
      cost_usd: 0,
    };
  }

  // Select implementation agent based on scope
  const agents = registry.getForWorkflow("implement");
  const agent = agents[0]; // Default to rust-backend for now
  if (!agent) {
    ctx.worktrees.remove(ctx.task.id);
    return { success: false, output: "No implementation agents found", cost_usd: 0 };
  }

  const tools = buildToolSet("implement");

  // Retrieve relevant knowledge context
  let kbContext = "";
  if (ctx.knowledgeRetriever) {
    const kb = await ctx.knowledgeRetriever.getContext(taskDescription, ["decision", "learning", "task_summary"]);
    kbContext = kb.formatted;
  }

  const prompt = `You are implementing a feature in the Harbangan codebase.

## Task
${taskDescription}
${kbContext ? `\n${kbContext}\n` : ""}

## Working Directory
You are working in: ${worktreePath}
Branch: ${branchName}

## Instructions
1. Read and understand the relevant code before making changes
2. Follow the project's coding conventions (see CLAUDE.md)
3. Implement the changes incrementally
4. Write tests following TDD where appropriate
5. Ensure all quality gates pass (cargo clippy, cargo test --lib, cargo fmt)
6. Commit your changes with a descriptive message following the commit convention:
   type(scope): description

## Quality Standards
- Backend: cargo clippy --all-targets (zero warnings), cargo fmt --check, cargo test --lib
- Frontend: npm run build, npm run lint
- Never use .unwrap() in production Rust code
- Never hardcode model IDs
- Follow import ordering: std → external → crate::

## Commit Convention
type(scope): description
Types: feat, fix, refactor, test, docs, chore
Scope: backend, frontend, streaming, auth, converter, etc.
Include Co-Authored-By: Claude <noreply@anthropic.com>

After completing implementation, commit all changes.`;

  const result = await executeAgent(agent, prompt, tools, ctx, worktreePath);

  // Run quality gates
  let gatesOutput = "";
  try {
    const gates = harbangan.runQualityGates(worktreePath, "backend");
    const failed = gates.filter((g) => !g.passed);
    gatesOutput = gates
      .map((g) => `${g.gate}: ${g.passed ? "PASS" : "FAIL"}`)
      .join("\n");

    if (failed.length > 0) {
      log.warn({ taskId: ctx.task.id, failed: failed.map((f) => f.gate) }, "Quality gates failed");
    }
  } catch (err) {
    log.warn({ error: String(err) }, "Quality gates execution failed");
  }

  // Push and create PR if successful
  let prUrl: string | undefined;
  if (result.success) {
    try {
      ctx.worktrees.commitAll(
        ctx.task.id,
        `feat: implement task ${ctx.task.id.slice(0, 8)}\n\nCo-Authored-By: Claude <noreply@anthropic.com>`,
      );
      ctx.worktrees.pushBranch(ctx.task.id);

      const prTitle = issueNumber
        ? `feat: implement #${issueNumber}`
        : `feat: ${(description ?? "implementation").slice(0, 60)}`;

      const prBody = `## Summary\n${description ?? taskDescription.slice(0, 500)}\n\n## Quality Gates\n\`\`\`\n${gatesOutput}\n\`\`\`\n\n${issueNumber ? `Closes #${issueNumber}` : ""}\n\n🤖 Generated by Harbangan Orchestrator`;

      prUrl = github.createPr(prTitle, prBody, branchName);
      log.info({ taskId: ctx.task.id, prUrl }, "PR created");
    } catch (err) {
      log.error({ error: String(err) }, "Failed to push/create PR");
    }
  }

  // Clean up worktree
  try {
    ctx.worktrees.remove(ctx.task.id);
  } catch (err) {
    log.warn({ error: String(err) }, "Failed to remove worktree");
  }

  return {
    ...result,
    output: `${result.output}\n\n---\nQuality Gates:\n${gatesOutput}`,
    branch: branchName,
    pr_url: prUrl,
  };
}
