import { query } from "@anthropic-ai/claude-agent-sdk";
import type { McpSdkServerConfigWithInstance } from "@anthropic-ai/claude-agent-sdk";
import { createChildLogger } from "../util/logger.js";
import type { AgentDefinition } from "../agents/registry.js";
import type { ITaskStore, Task } from "../store/types.js";
import type { AuditLogger } from "../safety/audit.js";
import type { SafetyGuardrails } from "../safety/guardrails.js";
import type { WorktreeManager } from "../workspace/worktree.js";
import type { KnowledgeRetriever } from "../knowledge/retrieval.js";
import type { InteractionBridge } from "../slack/interaction-bridge.js";
import type { SlackService } from "../slack/service.js";

const log = createChildLogger("workflow");

export interface WorkflowContext {
  task: Task;
  store: ITaskStore;
  audit: AuditLogger;
  guardrails: SafetyGuardrails;
  worktrees: WorktreeManager;
  repoPath: string;
  knowledgeRetriever: KnowledgeRetriever | null;
  interactionBridge: InteractionBridge | null;
  slackService: SlackService | null;
}

export interface WorkflowResult {
  success: boolean;
  output: string;
  cost_usd: number;
  branch?: string;
  pr_url?: string;
}

/**
 * Execute a Claude Agent SDK query with the given agent definition and prompt.
 * Handles streaming, cost tracking, and audit logging.
 */
export interface ExecuteAgentOptions {
  workdir?: string;
  mcpServers?: Record<string, McpSdkServerConfigWithInstance>;
}

export async function executeAgent(
  agent: AgentDefinition,
  prompt: string,
  tools: string[],
  ctx: WorkflowContext,
  workdirOrOptions?: string | ExecuteAgentOptions,
): Promise<WorkflowResult> {
  // Support both old (workdir string) and new (options object) signatures
  const opts: ExecuteAgentOptions = typeof workdirOrOptions === "string"
    ? { workdir: workdirOrOptions }
    : workdirOrOptions ?? {};
  const { workdir, mcpServers } = opts;
  let output = "";
  let costUsd = 0;
  let turns = 0;

  log.info(
    { taskId: ctx.task.id, agent: agent.name, workdir },
    "Executing agent",
  );

  try {
    for await (const message of query({
      prompt,
      options: {
        systemPrompt: agent.systemPrompt,
        allowedTools: tools,
        permissionMode: "bypassPermissions",
        maxTurns: agent.maxTurns,
        maxBudgetUsd: ctx.task.budget_usd,
        model: agent.model,
        ...(workdir ? { cwd: workdir } : {}),
        ...(mcpServers ? { mcpServers } : {}),
      },
    })) {
      if (message.type === "assistant" && message.message?.content) {
        for (const block of message.message.content) {
          if ("text" in block) {
            output += block.text + "\n";
          }
        }
      }

      if (message.type === "result") {
        costUsd = message.total_cost_usd ?? 0;
        turns = message.num_turns ?? 0;

        if (message.subtype === "success") {
          const result = message.result ?? output;
          log.info(
            { taskId: ctx.task.id, agent: agent.name, cost: costUsd, turns },
            "Agent completed successfully",
          );
          return { success: true, output: result, cost_usd: costUsd };
        }

        // Handle non-success subtypes
        const reason = message.subtype ?? "unknown";
        log.warn(
          { taskId: ctx.task.id, agent: agent.name, reason, cost: costUsd },
          "Agent finished with non-success status",
        );
        return {
          success: false,
          output: output || `Agent stopped: ${reason}`,
          cost_usd: costUsd,
        };
      }
    }
  } catch (err) {
    const errMsg = err instanceof Error ? err.message : String(err);
    log.error(
      { taskId: ctx.task.id, agent: agent.name, error: errMsg },
      "Agent execution failed",
    );
    return { success: false, output: errMsg, cost_usd: costUsd };
  }

  return { success: false, output: output || "No result", cost_usd: costUsd };
}
