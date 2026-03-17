import type { WorkflowType, AgentInfo } from "../store/types.js";
import { rustBackendAgent } from "./definitions/rust-backend.js";
import { reactFrontendAgent } from "./definitions/react-frontend.js";
import { databaseAgent } from "./definitions/database.js";
import { devopsAgent } from "./definitions/devops.js";
import { backendQaAgent } from "./definitions/backend-qa.js";
import { frontendQaAgent } from "./definitions/frontend-qa.js";
import { docWriterAgent } from "./definitions/doc-writer.js";
import { kanbanAgent } from "./definitions/kanban.js";

export interface AgentDefinition {
  name: string;
  description: string;
  model: string;
  maxTurns: number;
  workflows: WorkflowType[];
  systemPrompt: string;
  fileOwnership: string[];
}

/** Tool sets granted per workflow type */
export const WORKFLOW_TOOLS: Record<WorkflowType, string[]> = {
  plan: ["Read", "Grep", "Glob", "Bash"],
  "pr-review": ["Read", "Grep", "Glob", "Bash"],
  implement: ["Read", "Write", "Edit", "Bash", "Grep", "Glob"],
  kanban: ["Read", "Bash", "Grep", "Glob"],
  docs: ["Read", "Write", "Edit", "Grep", "Glob", "Bash"],
  qa: ["Read", "Write", "Edit", "Bash", "Grep", "Glob"],
};

/** Agents selected per scope */
export type Scope =
  | "backend"
  | "frontend"
  | "fullstack"
  | "database"
  | "infrastructure"
  | "documentation"
  | "board";

export const SCOPE_AGENTS: Record<Scope, string[]> = {
  backend: ["rust-backend-engineer", "backend-qa"],
  frontend: ["react-frontend-engineer", "frontend-qa"],
  fullstack: ["rust-backend-engineer", "react-frontend-engineer", "backend-qa"],
  database: ["database-engineer", "rust-backend-engineer"],
  infrastructure: ["devops-engineer", "rust-backend-engineer"],
  documentation: ["doc-writer"],
  board: ["kanban-master"],
};

const ALL_AGENTS: AgentDefinition[] = [
  rustBackendAgent,
  reactFrontendAgent,
  databaseAgent,
  devopsAgent,
  backendQaAgent,
  frontendQaAgent,
  docWriterAgent,
  kanbanAgent,
];

export class AgentRegistry {
  private agents = new Map<string, AgentDefinition>();

  constructor() {
    for (const agent of ALL_AGENTS) {
      this.agents.set(agent.name, agent);
    }
  }

  get(name: string): AgentDefinition | undefined {
    return this.agents.get(name);
  }

  getForWorkflow(workflowType: WorkflowType): AgentDefinition[] {
    return ALL_AGENTS.filter((a) => a.workflows.includes(workflowType));
  }

  getForScope(scope: Scope): AgentDefinition[] {
    const names = SCOPE_AGENTS[scope] ?? [];
    return names
      .map((n) => this.agents.get(n))
      .filter((a): a is AgentDefinition => a !== undefined);
  }

  getToolsForWorkflow(workflowType: WorkflowType): string[] {
    return WORKFLOW_TOOLS[workflowType] ?? [];
  }

  listAll(): AgentInfo[] {
    return ALL_AGENTS.map((a) => ({
      name: a.name,
      description: a.description,
      model: a.model,
      workflows: a.workflows,
    }));
  }

  get size(): number {
    return this.agents.size;
  }
}
