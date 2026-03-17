import Database from "better-sqlite3";
import path from "node:path";
import { v4 as uuidv4 } from "uuid";
import { createChildLogger } from "../util/logger.js";
import type {
  AuditEntry,
  BudgetUsage,
  ITaskStore,
  Priority,
  Task,
  TaskInput,
  TaskStats,
  TaskStatus,
  WorkflowType,
} from "./types.js";

const log = createChildLogger("task-store");

export class SqliteTaskStore implements ITaskStore {
  private db: Database.Database;

  constructor(dataDir: string) {
    const dbPath = path.join(dataDir, "orchestrator.db");
    this.db = new Database(dbPath);
    this.db.pragma("journal_mode = WAL");
    this.db.pragma("foreign_keys = ON");
    this.migrate();
    log.info({ dbPath }, "Task store initialized");
  }

  private migrate(): void {
    this.db.exec(`
      CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        type TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        priority TEXT NOT NULL DEFAULT 'normal',
        input TEXT NOT NULL,
        budget_usd REAL NOT NULL DEFAULT 10,
        cost_usd REAL NOT NULL DEFAULT 0,
        output TEXT,
        error TEXT,
        branch TEXT,
        pr_url TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        started_at TEXT,
        completed_at TEXT,
        session_id TEXT
      );

      CREATE TABLE IF NOT EXISTS audit_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        task_id TEXT NOT NULL,
        timestamp TEXT NOT NULL DEFAULT (datetime('now')),
        tool_name TEXT NOT NULL,
        tool_input_summary TEXT NOT NULL DEFAULT '',
        allowed INTEGER NOT NULL DEFAULT 1,
        cost_usd REAL NOT NULL DEFAULT 0,
        FOREIGN KEY (task_id) REFERENCES tasks(id)
      );

      CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
      CREATE INDEX IF NOT EXISTS idx_tasks_type ON tasks(type);
      CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at);
      CREATE INDEX IF NOT EXISTS idx_audit_task ON audit_log(task_id);
      CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
    `);
  }

  createTask(
    type: WorkflowType,
    input: TaskInput,
    priority: Priority = "normal",
    budgetUsd: number = 10,
  ): Task {
    const id = uuidv4();
    const now = new Date().toISOString();

    this.db
      .prepare(
        `INSERT INTO tasks (id, type, status, priority, input, budget_usd, created_at)
       VALUES (?, ?, 'pending', ?, ?, ?, ?)`,
      )
      .run(id, type, priority, JSON.stringify(input), budgetUsd, now);

    return this.getTask(id)!;
  }

  getTask(id: string): Task | null {
    const row = this.db
      .prepare("SELECT * FROM tasks WHERE id = ?")
      .get(id) as Record<string, unknown> | undefined;

    if (!row) return null;
    return this.rowToTask(row);
  }

  listTasks(filters?: {
    status?: TaskStatus;
    type?: WorkflowType;
    limit?: number;
    offset?: number;
  }): Task[] {
    const conditions: string[] = [];
    const params: unknown[] = [];

    if (filters?.status) {
      conditions.push("status = ?");
      params.push(filters.status);
    }
    if (filters?.type) {
      conditions.push("type = ?");
      params.push(filters.type);
    }

    const where =
      conditions.length > 0 ? `WHERE ${conditions.join(" AND ")}` : "";
    const limit = filters?.limit ?? 50;
    const offset = filters?.offset ?? 0;

    const rows = this.db
      .prepare(
        `SELECT * FROM tasks ${where} ORDER BY created_at DESC LIMIT ? OFFSET ?`,
      )
      .all(...params, limit, offset) as Record<string, unknown>[];

    return rows.map((r) => this.rowToTask(r));
  }

  updateTaskStatus(
    id: string,
    status: TaskStatus,
    extra?: Partial<
      Pick<Task, "output" | "error" | "branch" | "pr_url" | "cost_usd" | "session_id">
    >,
  ): void {
    const sets = ["status = ?"];
    const params: unknown[] = [status];

    if (status === "running") {
      sets.push("started_at = ?");
      params.push(new Date().toISOString());
    }
    if (status === "completed" || status === "failed" || status === "cancelled") {
      sets.push("completed_at = ?");
      params.push(new Date().toISOString());
    }
    if (extra?.output !== undefined) {
      sets.push("output = ?");
      params.push(extra.output);
    }
    if (extra?.error !== undefined) {
      sets.push("error = ?");
      params.push(extra.error);
    }
    if (extra?.branch !== undefined) {
      sets.push("branch = ?");
      params.push(extra.branch);
    }
    if (extra?.pr_url !== undefined) {
      sets.push("pr_url = ?");
      params.push(extra.pr_url);
    }
    if (extra?.cost_usd !== undefined) {
      sets.push("cost_usd = ?");
      params.push(extra.cost_usd);
    }
    if (extra?.session_id !== undefined) {
      sets.push("session_id = ?");
      params.push(extra.session_id);
    }

    params.push(id);
    this.db
      .prepare(`UPDATE tasks SET ${sets.join(", ")} WHERE id = ?`)
      .run(...params);
  }

  addAuditEntry(
    taskId: string,
    toolName: string,
    toolInputSummary: string,
    allowed: boolean,
    costUsd: number = 0,
  ): void {
    this.db
      .prepare(
        `INSERT INTO audit_log (task_id, tool_name, tool_input_summary, allowed, cost_usd)
       VALUES (?, ?, ?, ?, ?)`,
      )
      .run(taskId, toolName, toolInputSummary, allowed ? 1 : 0, costUsd);
  }

  getAuditLog(taskId: string, limit: number = 100): AuditEntry[] {
    return this.db
      .prepare(
        "SELECT * FROM audit_log WHERE task_id = ? ORDER BY timestamp DESC LIMIT ?",
      )
      .all(taskId, limit) as AuditEntry[];
  }

  getDailySpend(): number {
    const today = new Date().toISOString().split("T")[0];
    const row = this.db
      .prepare(
        `SELECT COALESCE(SUM(cost_usd), 0) as total
       FROM tasks WHERE created_at >= ? AND status IN ('completed', 'running')`,
      )
      .get(today) as { total: number };
    return row.total;
  }

  getMonthlySpend(): number {
    const now = new Date();
    const monthStart = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-01`;
    const row = this.db
      .prepare(
        `SELECT COALESCE(SUM(cost_usd), 0) as total
       FROM tasks WHERE created_at >= ? AND status IN ('completed', 'running')`,
      )
      .get(monthStart) as { total: number };
    return row.total;
  }

  getBudgetUsage(
    dailyLimit: number,
    monthlyLimit: number,
  ): BudgetUsage {
    const daily = this.getDailySpend();
    const monthly = this.getMonthlySpend();
    return {
      daily_usd: daily,
      monthly_usd: monthly,
      daily_limit_usd: dailyLimit,
      monthly_limit_usd: monthlyLimit,
      daily_remaining_usd: Math.max(0, dailyLimit - daily),
      monthly_remaining_usd: Math.max(0, monthlyLimit - monthly),
    };
  }

  getStats(dailyLimit: number, monthlyLimit: number): TaskStats {
    const totals = this.db
      .prepare(
        `SELECT
        COUNT(*) as total,
        SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
        SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed,
        SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) as cancelled,
        COALESCE(SUM(cost_usd), 0) as total_cost
       FROM tasks`,
      )
      .get() as {
      total: number;
      completed: number;
      failed: number;
      cancelled: number;
      total_cost: number;
    };

    return {
      total_tasks: totals.total,
      completed_tasks: totals.completed,
      failed_tasks: totals.failed,
      cancelled_tasks: totals.cancelled,
      total_cost_usd: totals.total_cost,
      budget_usage: this.getBudgetUsage(dailyLimit, monthlyLimit),
    };
  }

  getActiveTasks(): Task[] {
    return this.listTasks({ status: "running" });
  }

  getQueuedTasks(): Task[] {
    return this.listTasks({ status: "queued" });
  }

  close(): void {
    this.db.close();
  }

  private rowToTask(row: Record<string, unknown>): Task {
    return {
      id: row.id as string,
      type: row.type as WorkflowType,
      status: row.status as TaskStatus,
      priority: row.priority as Priority,
      input: JSON.parse(row.input as string) as TaskInput,
      budget_usd: row.budget_usd as number,
      cost_usd: row.cost_usd as number,
      output: row.output as string | null,
      error: row.error as string | null,
      branch: row.branch as string | null,
      pr_url: row.pr_url as string | null,
      created_at: row.created_at as string,
      started_at: row.started_at as string | null,
      completed_at: row.completed_at as string | null,
      session_id: row.session_id as string | null,
    };
  }
}

/** @deprecated Use SqliteTaskStore directly. Kept for backwards compatibility. */
export const TaskStore = SqliteTaskStore;
