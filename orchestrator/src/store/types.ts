export type TaskStatus =
  | "pending"
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export type WorkflowType =
  | "plan"
  | "pr-review"
  | "implement"
  | "kanban"
  | "docs"
  | "qa";

export type Priority = "high" | "normal" | "low";

export interface TaskInput {
  description?: string;
  pr_number?: number;
  issue_number?: number;
  plan_task_id?: string;
  command?: string;
  scope?: string;
  target?: string;
  budget_usd?: number;
  slack_channel_id?: string;
  slack_thread_ts?: string;
  auto_fix?: boolean;
}

export interface Task {
  id: string;
  type: WorkflowType;
  status: TaskStatus;
  priority: Priority;
  input: TaskInput;
  budget_usd: number;
  cost_usd: number;
  output: string | null;
  error: string | null;
  branch: string | null;
  pr_url: string | null;
  created_at: string;
  started_at: string | null;
  completed_at: string | null;
  session_id: string | null;
}

export interface AuditEntry {
  id: number;
  task_id: string;
  timestamp: string;
  tool_name: string;
  tool_input_summary: string;
  allowed: boolean;
  cost_usd: number;
}

export interface BudgetUsage {
  daily_usd: number;
  monthly_usd: number;
  daily_limit_usd: number;
  monthly_limit_usd: number;
  daily_remaining_usd: number;
  monthly_remaining_usd: number;
}

export interface AgentInfo {
  name: string;
  description: string;
  model: string;
  workflows: WorkflowType[];
}

export interface HealthStatus {
  status: "healthy" | "degraded" | "unhealthy";
  uptime_seconds: number;
  workspace_ready: boolean;
  queue_size: number;
  active_tasks: number;
  version: string;
}

export interface TaskStats {
  total_tasks: number;
  completed_tasks: number;
  failed_tasks: number;
  cancelled_tasks: number;
  total_cost_usd: number;
  budget_usage: BudgetUsage;
}

/**
 * Abstract interface for task storage backends.
 * Reads are sync (cache-backed). Writes may be async (GitHub API) or sync (SQLite).
 */
export interface ITaskStore {
  // --- Reads (sync, cache-backed) ---
  getTask(id: string): Task | null;
  listTasks(filters?: {
    status?: TaskStatus;
    type?: WorkflowType;
    limit?: number;
    offset?: number;
  }): Task[];
  getActiveTasks(): Task[];
  getQueuedTasks(): Task[];
  getAuditLog(taskId: string, limit?: number): AuditEntry[];
  getDailySpend(): number;
  getMonthlySpend(): number;
  getBudgetUsage(dailyLimit: number, monthlyLimit: number): BudgetUsage;
  getStats(dailyLimit: number, monthlyLimit: number): TaskStats;

  // --- Writes ---
  createTask(
    type: WorkflowType,
    input: TaskInput,
    priority?: Priority,
    budgetUsd?: number,
  ): Task;

  updateTaskStatus(
    id: string,
    status: TaskStatus,
    extra?: Partial<
      Pick<Task, "output" | "error" | "branch" | "pr_url" | "cost_usd" | "session_id">
    >,
  ): void;

  addAuditEntry(
    taskId: string,
    toolName: string,
    toolInputSummary: string,
    allowed: boolean,
    costUsd?: number,
  ): void;

  // --- Lifecycle ---
  close(): void;
}
