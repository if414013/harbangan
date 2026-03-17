import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";

const log = createChildLogger("workflow:docs");

/**
 * Documentation workflow: generates or updates documentation.
 *
 * Scopes: api-reference, architecture, release-notes, deployment-guide
 */
export async function executeDocsWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
): Promise<WorkflowResult> {
  const scope = ctx.task.input.scope ?? "api-reference";
  const description = ctx.task.input.description ?? `Generate ${scope} documentation`;

  log.info({ taskId: ctx.task.id, scope }, "Starting docs workflow");

  const agents = registry.getForWorkflow("docs");
  const agent = agents[0];
  if (!agent) {
    return { success: false, output: "No documentation agents found", cost_usd: 0 };
  }

  const tools = buildToolSet("docs");

  // Retrieve relevant knowledge context
  let kbContext = "";
  if (ctx.knowledgeRetriever) {
    const kb = await ctx.knowledgeRetriever.getContext(description, ["decision", "task_summary"]);
    kbContext = kb.formatted;
  }

  const prompt = `You are writing documentation for the Harbangan API gateway.

## Scope
${scope}

## Task
${description}
${kbContext ? `\n${kbContext}\n` : ""}

## Instructions
1. Read the relevant source code to ensure accuracy
2. Follow Harbangan's documentation standards:
   - Clear overview section
   - Hierarchical headings (max 3 levels)
   - Code blocks with language tags
   - Real examples from the codebase
   - Tables for structured data
3. Reference actual file paths, endpoints, and code patterns
4. Never guess — verify from source code

## Architecture Context
- Backend: Rust/Axum 0.7, Tokio, sqlx 0.8, PostgreSQL 16
- Frontend: React 19, TypeScript 5.9, Vite 7
- API: OpenAI-compatible (/v1/chat/completions) and Anthropic-compatible (/v1/messages)
- Auth: API key for proxy, Google SSO or password+TOTP for web UI
- Two deployment modes: full and proxy-only

## Output
Produce the documentation as markdown. Include all relevant details for the requested scope.`;

  return executeAgent(agent, prompt, tools, ctx, ctx.repoPath);
}
