import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import { buildToolSet } from "../agents/tools.js";
import { HarbanganTools } from "../mcp/harbangan-tools.js";
import type { WorkflowContext, WorkflowResult } from "./base.js";
import { executeAgent } from "./base.js";

const log = createChildLogger("workflow:qa");

/**
 * QA/Testing workflow: run tests, identify gaps, optionally write new tests.
 *
 * Scopes: backend, frontend, both
 */
export async function executeQaWorkflow(
  ctx: WorkflowContext,
  registry: AgentRegistry,
): Promise<WorkflowResult> {
  const scope = (ctx.task.input.scope ?? "backend") as
    | "backend"
    | "frontend"
    | "both";
  const target = ctx.task.input.target;
  const description = ctx.task.input.description;

  log.info({ taskId: ctx.task.id, scope, target }, "Starting QA workflow");

  // First run quality gates to get baseline
  const harbangan = new HarbanganTools();
  let gatesOutput = "";
  try {
    const gates = harbangan.runQualityGates(ctx.repoPath, scope);
    gatesOutput = gates
      .map(
        (g) =>
          `${g.gate}: ${g.passed ? "PASS" : "FAIL"} (${g.duration_ms}ms)`,
      )
      .join("\n");
  } catch (err) {
    gatesOutput = `Quality gate execution failed: ${String(err)}`;
  }

  // Select QA agent
  const agents = registry.getForWorkflow("qa");
  const agent = agents[0];
  if (!agent) {
    return {
      success: false,
      output: "No QA agents found",
      cost_usd: 0,
    };
  }

  const tools = buildToolSet("qa");

  // Retrieve relevant knowledge context
  let kbContext = "";
  if (ctx.knowledgeRetriever) {
    const queryText = description ?? `QA analysis for ${scope}`;
    const kb = await ctx.knowledgeRetriever.getContext(queryText, ["learning", "incident"]);
    kbContext = kb.formatted;
  }

  const prompt = `You are running QA analysis on the Harbangan codebase.

## Scope
${scope}${target ? ` — Target: ${target}` : ""}
${description ? `\nDescription: ${description}` : ""}
${kbContext ? `\n${kbContext}\n` : ""}

## Current Quality Gate Results
\`\`\`
${gatesOutput}
\`\`\`

## Instructions
1. Analyze the current test suite for the specified scope
2. Identify coverage gaps — modules, functions, or paths without tests
3. Check for common test antipatterns:
   - Missing edge cases
   - Missing error path tests
   - Tests that don't actually assert anything meaningful
   - Flaky test patterns
4. If quality gates failed, diagnose the root cause
5. Optionally write new tests to fill critical gaps

## Testing Standards
- Backend: #[cfg(test)] mod tests, test_<func>_<scenario>, #[tokio::test]
- Frontend: Playwright E2E in e2e-tests/
- Helper configs: create_test_config() / Config::with_defaults()
- Always include edge cases and error scenarios

## Output
Provide:
1. Quality gate summary
2. Coverage analysis
3. Gap identification (prioritized)
4. Recommendations
5. Any new tests written`;

  const result = await executeAgent(agent, prompt, tools, ctx, ctx.repoPath);

  return {
    ...result,
    output: `## Quality Gates\n\`\`\`\n${gatesOutput}\n\`\`\`\n\n${result.output}`,
  };
}
