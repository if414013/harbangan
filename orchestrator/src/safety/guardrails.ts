import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("guardrails");

/** Patterns that are always blocked, regardless of context */
const BLOCKED_PATTERNS = [
  // Destructive git operations
  { pattern: /git\s+push\s+.*main/, reason: "Direct push to main blocked" },
  { pattern: /git\s+push\s+.*master/, reason: "Direct push to master blocked" },
  { pattern: /git\s+push\s+.*--force/, reason: "Force push blocked" },
  { pattern: /git\s+push\s+-f\b/, reason: "Force push blocked" },
  { pattern: /git\s+reset\s+--hard/, reason: "Hard reset blocked" },

  // Destructive file operations
  { pattern: /rm\s+-rf\s+\//, reason: "Recursive delete of root blocked" },
  { pattern: /rm\s+-rf\s+~/, reason: "Recursive delete of home blocked" },
  { pattern: /rm\s+-rf\s+\$HOME/, reason: "Recursive delete of home blocked" },
  { pattern: /rm\s+-rf\s+\.\./, reason: "Recursive delete outside workspace blocked" },

  // Credential exposure
  {
    pattern: /echo\s+.*(?:password|secret|token|key)/i,
    reason: "Potential credential exposure blocked",
  },

  // Network operations that shouldn't happen
  { pattern: /curl\s+.*\|\s*(?:bash|sh)/, reason: "Remote script execution blocked" },
  { pattern: /wget\s+.*\|\s*(?:bash|sh)/, reason: "Remote script execution blocked" },
];

export interface GuardrailCheck {
  allowed: boolean;
  reason?: string;
}

export class SafetyGuardrails {
  checkToolUse(toolName: string, toolInput: string): GuardrailCheck {
    // Only check Bash commands
    if (toolName !== "Bash" && toolName !== "bash") {
      return { allowed: true };
    }

    for (const { pattern, reason } of BLOCKED_PATTERNS) {
      if (pattern.test(toolInput)) {
        log.warn({ toolName, reason, input: toolInput.slice(0, 200) }, "Tool use blocked");
        return { allowed: false, reason };
      }
    }

    return { allowed: true };
  }

  /** Validate that a branch name is safe */
  validateBranchName(branch: string): GuardrailCheck {
    if (branch === "main" || branch === "master") {
      return { allowed: false, reason: "Cannot operate directly on main/master" };
    }
    if (!/^[a-zA-Z0-9/_.-]+$/.test(branch)) {
      return { allowed: false, reason: "Invalid branch name characters" };
    }
    return { allowed: true };
  }
}
