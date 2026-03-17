import type { WorkflowType } from "../store/types.js";
import { WORKFLOW_TOOLS } from "./registry.js";

/**
 * Build the allowedTools list for a given workflow type.
 * Includes base tools plus any MCP tool names.
 */
export function buildToolSet(
  workflowType: WorkflowType,
  mcpToolNames: string[] = [],
): string[] {
  const base = WORKFLOW_TOOLS[workflowType] ?? [];
  return [...base, ...mcpToolNames];
}

/**
 * Build disallowed tools for safety.
 * These patterns are always blocked regardless of workflow.
 */
export function buildDisallowedTools(): string[] {
  return [
    "Bash(rm -rf /)",
    "Bash(rm -rf ~)",
    "Bash(sudo)",
  ];
}
