import { execSync } from "node:child_process";
import { createChildLogger } from "../util/logger.js";
import { GitHubProjects } from "./github-projects.js";
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

const log = createChildLogger("github-task-store");

const REPO = "if414013/harbangan";

/** Map TaskStatus → board column status name */
const STATUS_TO_BOARD: Record<TaskStatus, string> = {
  pending: "pending",
  queued: "queued",
  running: "running",
  completed: "completed",
  failed: "failed",
  cancelled: "cancelled",
};

interface CachedIssue {
  task: Task;
  issueNodeId: string;
  projectItemId: string | null;
}

/**
 * GitHub-backed task store. Uses GitHub Issues + Projects V2 for persistence
 * with an in-memory cache for sync reads.
 *
 * Architecture:
 * - Reads: instant from Map cache (sync)
 * - Writes: cache first (sync), then async fire-and-forget to GitHub
 * - Audit: buffered in memory, flushed as issue comment on task completion
 */
export class GitHubTaskStore implements ITaskStore {
  private cache = new Map<string, CachedIssue>();
  private auditBuffer = new Map<string, AuditEntry[]>();
  private ghToken: string;
  private projects: GitHubProjects;
  private nextAuditId = 1;

  constructor(ghToken: string, projects: GitHubProjects) {
    this.ghToken = ghToken;
    this.projects = projects;
  }

  /** Load all orchestrator-labeled issues into cache. Call on startup. */
  async initialize(): Promise<void> {
    try {
      const output = this.gh(
        `issue list --repo ${REPO} --label orchestrator --state all --limit 200 --json number,title,body,state,labels,createdAt,closedAt`,
      );
      const issues = JSON.parse(output) as Array<{
        number: number;
        title: string;
        body: string;
        state: string;
        labels: Array<{ name: string }>;
        createdAt: string;
        closedAt: string | null;
      }>;

      for (const issue of issues) {
        const task = this.issueToTask(issue);
        if (task) {
          this.cache.set(task.id, {
            task,
            issueNodeId: "", // Will be resolved lazily
            projectItemId: null,
          });
        }
      }

      log.info({ count: this.cache.size }, "Loaded orchestrator tasks from GitHub");
    } catch (err) {
      log.error({ error: String(err) }, "Failed to load tasks from GitHub");
    }
  }

  // --- Sync reads (cache-backed) ---

  getTask(id: string): Task | null {
    return this.cache.get(id)?.task ?? null;
  }

  listTasks(filters?: {
    status?: TaskStatus;
    type?: WorkflowType;
    limit?: number;
    offset?: number;
  }): Task[] {
    let tasks = Array.from(this.cache.values()).map((c) => c.task);

    if (filters?.status) {
      tasks = tasks.filter((t) => t.status === filters.status);
    }
    if (filters?.type) {
      tasks = tasks.filter((t) => t.type === filters.type);
    }

    // Sort by created_at descending
    tasks.sort((a, b) => b.created_at.localeCompare(a.created_at));

    const offset = filters?.offset ?? 0;
    const limit = filters?.limit ?? 50;
    return tasks.slice(offset, offset + limit);
  }

  getActiveTasks(): Task[] {
    return this.listTasks({ status: "running" });
  }

  getQueuedTasks(): Task[] {
    return this.listTasks({ status: "queued" });
  }

  getAuditLog(taskId: string, limit: number = 100): AuditEntry[] {
    const entries = this.auditBuffer.get(taskId) ?? [];
    return entries.slice(0, limit);
  }

  getDailySpend(): number {
    const today = new Date().toISOString().split("T")[0]!;
    let total = 0;
    for (const { task } of this.cache.values()) {
      if (
        task.created_at >= today &&
        (task.status === "completed" || task.status === "running")
      ) {
        total += task.cost_usd;
      }
    }
    return total;
  }

  getMonthlySpend(): number {
    const now = new Date();
    const monthStart = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-01`;
    let total = 0;
    for (const { task } of this.cache.values()) {
      if (
        task.created_at >= monthStart &&
        (task.status === "completed" || task.status === "running")
      ) {
        total += task.cost_usd;
      }
    }
    return total;
  }

  getBudgetUsage(dailyLimit: number, monthlyLimit: number): BudgetUsage {
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
    let total = 0;
    let completed = 0;
    let failed = 0;
    let cancelled = 0;
    let totalCost = 0;

    for (const { task } of this.cache.values()) {
      total++;
      if (task.status === "completed") completed++;
      if (task.status === "failed") failed++;
      if (task.status === "cancelled") cancelled++;
      totalCost += task.cost_usd;
    }

    return {
      total_tasks: total,
      completed_tasks: completed,
      failed_tasks: failed,
      cancelled_tasks: cancelled,
      total_cost_usd: totalCost,
      budget_usage: this.getBudgetUsage(dailyLimit, monthlyLimit),
    };
  }

  // --- Writes (sync cache + async GitHub) ---

  createTask(
    type: WorkflowType,
    input: TaskInput,
    priority: Priority = "normal",
    budgetUsd: number = 10,
  ): Task {
    const now = new Date().toISOString();

    // Create GitHub issue synchronously (blocks briefly, but needed for ID)
    const description = input.description ?? `${type} task`;
    const title = `[orchestrator:${type}] ${description.slice(0, 80)}`;
    const body = this.formatIssueBody(type, input, budgetUsd);
    const labels = ["orchestrator", `type:${type}`, `priority:${priority}`];
    const labelArgs = labels.map((l) => `--label "${l}"`).join(" ");

    let issueNumber: string;
    let issueNodeId = "";
    try {
      const output = this.gh(
        `issue create --repo ${REPO} --title "${this.escape(title)}" --body "${this.escape(body)}" ${labelArgs} --json number,nodeId`,
      );
      const parsed = JSON.parse(output) as { number: number; nodeId: string };
      issueNumber = String(parsed.number);
      issueNodeId = parsed.nodeId;
    } catch (err) {
      // Fallback: create without JSON output, parse URL
      try {
        const url = this.gh(
          `issue create --repo ${REPO} --title "${this.escape(title)}" --body "${this.escape(body)}" ${labelArgs}`,
        );
        const match = url.match(/\/(\d+)$/);
        issueNumber = match ? match[1]! : `local-${Date.now()}`;
      } catch (innerErr) {
        log.error({ error: String(innerErr) }, "Failed to create GitHub issue");
        issueNumber = `local-${Date.now()}`;
      }
    }

    const task: Task = {
      id: issueNumber,
      type,
      status: "pending",
      priority,
      input,
      budget_usd: budgetUsd,
      cost_usd: 0,
      output: null,
      error: null,
      branch: null,
      pr_url: null,
      created_at: now,
      started_at: null,
      completed_at: null,
      session_id: null,
    };

    this.cache.set(issueNumber, { task, issueNodeId, projectItemId: null });

    // Async: add to project and set fields
    this.addToProjectAsync(issueNumber, issueNodeId, type, priority, budgetUsd);

    log.info({ taskId: issueNumber, type, priority }, "Task created as GitHub issue");
    return task;
  }

  updateTaskStatus(
    id: string,
    status: TaskStatus,
    extra?: Partial<
      Pick<Task, "output" | "error" | "branch" | "pr_url" | "cost_usd" | "session_id">
    >,
  ): void {
    const cached = this.cache.get(id);
    if (!cached) {
      log.warn({ taskId: id }, "Task not found in cache for status update");
      return;
    }

    // Update cache synchronously
    cached.task.status = status;
    if (status === "running" && !cached.task.started_at) {
      cached.task.started_at = new Date().toISOString();
    }
    if (status === "completed" || status === "failed" || status === "cancelled") {
      cached.task.completed_at = new Date().toISOString();
    }
    if (extra?.output !== undefined) cached.task.output = extra.output;
    if (extra?.error !== undefined) cached.task.error = extra.error;
    if (extra?.branch !== undefined) cached.task.branch = extra.branch;
    if (extra?.pr_url !== undefined) cached.task.pr_url = extra.pr_url;
    if (extra?.cost_usd !== undefined) cached.task.cost_usd = extra.cost_usd;
    if (extra?.session_id !== undefined) cached.task.session_id = extra.session_id;

    // Async: update GitHub issue + project fields
    this.updateGitHubAsync(id, status, extra);
  }

  addAuditEntry(
    taskId: string,
    toolName: string,
    toolInputSummary: string,
    allowed: boolean,
    costUsd: number = 0,
  ): void {
    const entry: AuditEntry = {
      id: this.nextAuditId++,
      task_id: taskId,
      timestamp: new Date().toISOString(),
      tool_name: toolName,
      tool_input_summary: toolInputSummary,
      allowed,
      cost_usd: costUsd,
    };

    let entries = this.auditBuffer.get(taskId);
    if (!entries) {
      entries = [];
      this.auditBuffer.set(taskId, entries);
    }
    entries.push(entry);
  }

  close(): void {
    // Flush any remaining audit buffers
    for (const [taskId, entries] of this.auditBuffer) {
      if (entries.length > 0) {
        this.flushAuditLog(taskId, entries);
      }
    }
    log.info("GitHub task store closed");
  }

  // --- Private: async GitHub operations ---

  private addToProjectAsync(
    issueNumber: string,
    issueNodeId: string,
    type: WorkflowType,
    _priority: Priority,
    budgetUsd: number,
  ): void {
    (async () => {
      try {
        // Resolve node ID if we don't have it
        if (!issueNodeId) {
          const output = this.gh(
            `issue view ${issueNumber} --repo ${REPO} --json nodeId`,
          );
          issueNodeId = (JSON.parse(output) as { nodeId: string }).nodeId;
        }

        const itemId = this.projects.addItemToProject(issueNodeId);

        const cached = this.cache.get(issueNumber);
        if (cached) {
          cached.issueNodeId = issueNodeId;
          cached.projectItemId = itemId;
        }

        this.projects.updateTaskFields(itemId, {
          status: STATUS_TO_BOARD[cached?.task.status ?? "pending"],
          type,
          budget: budgetUsd,
        });

        log.debug({ taskId: issueNumber, itemId }, "Issue added to project");
      } catch (err) {
        log.warn({ taskId: issueNumber, error: String(err) }, "Failed to add issue to project");
      }
    })();
  }

  private updateGitHubAsync(
    id: string,
    status: TaskStatus,
    extra?: Partial<
      Pick<Task, "output" | "error" | "branch" | "pr_url" | "cost_usd" | "session_id">
    >,
  ): void {
    (async () => {
      try {
        const cached = this.cache.get(id);
        const issueNumber = parseInt(id, 10);
        if (isNaN(issueNumber)) return;

        // Update project fields if we have a project item ID
        if (cached?.projectItemId) {
          const fieldUpdates: Record<string, unknown> = {
            status: STATUS_TO_BOARD[status],
          };
          if (extra?.cost_usd !== undefined) fieldUpdates.cost = extra.cost_usd;
          if (extra?.branch) fieldUpdates.branch = extra.branch;
          if (extra?.pr_url) fieldUpdates.prUrl = extra.pr_url;

          this.projects.updateTaskFields(
            cached.projectItemId,
            fieldUpdates as Parameters<GitHubProjects["updateTaskFields"]>[1],
          );
        }

        // Close issue on terminal states
        if (status === "completed" || status === "failed" || status === "cancelled") {
          // Add failure/cancel labels
          if (status === "failed") {
            try {
              this.gh(`issue edit ${issueNumber} --repo ${REPO} --add-label "orchestrator:failed"`);
            } catch { /* label may not exist */ }
          }
          if (status === "cancelled") {
            try {
              this.gh(`issue edit ${issueNumber} --repo ${REPO} --add-label "orchestrator:cancelled"`);
            } catch { /* label may not exist */ }
          }

          // Post output as comment
          if (extra?.output) {
            const comment = this.formatOutputComment(status, extra.output, extra.cost_usd);
            this.gh(
              `issue comment ${issueNumber} --repo ${REPO} --body "${this.escape(comment)}"`,
            );
          }

          // Flush audit log as comment
          const auditEntries = this.auditBuffer.get(id);
          if (auditEntries && auditEntries.length > 0) {
            this.flushAuditLog(id, auditEntries);
            this.auditBuffer.delete(id);
          }

          // Close the issue
          this.gh(`issue close ${issueNumber} --repo ${REPO}`);
        }
      } catch (err) {
        log.warn({ taskId: id, error: String(err) }, "Failed to update GitHub issue");
      }
    })();
  }

  private flushAuditLog(taskId: string, entries: AuditEntry[]): void {
    try {
      const issueNumber = parseInt(taskId, 10);
      if (isNaN(issueNumber)) return;

      const rows = entries
        .map(
          (e, i) =>
            `| ${i + 1} | ${e.tool_name} | ${e.allowed ? "pass" : "**DENIED**"} | $${e.cost_usd.toFixed(2)} | ${e.tool_input_summary.slice(0, 60)} |`,
        )
        .join("\n");

      const totalCost = entries.reduce((sum, e) => sum + e.cost_usd, 0);

      const comment = `## Audit Log (${entries.length} tool invocations)\n| # | Tool | Allowed | Cost | Summary |\n|---|------|---------|------|---------|\n${rows}\n\n**Total cost**: $${totalCost.toFixed(2)}`;

      this.gh(
        `issue comment ${issueNumber} --repo ${REPO} --body "${this.escape(comment)}"`,
      );

      log.debug({ taskId, entries: entries.length }, "Audit log flushed to GitHub");
    } catch (err) {
      log.warn({ taskId, error: String(err) }, "Failed to flush audit log");
    }
  }

  // --- Helpers ---

  private formatIssueBody(type: WorkflowType, input: TaskInput, budgetUsd: number): string {
    const metadata = JSON.stringify(input, null, 2);
    return [
      "## Task Input",
      "```json",
      metadata,
      "```",
      "",
      `**Type**: ${type}`,
      `**Budget**: $${budgetUsd.toFixed(2)}`,
      "",
      "_Created by Harbangan Orchestrator_",
    ].join("\n");
  }

  private formatOutputComment(
    status: TaskStatus,
    output: string,
    costUsd?: number,
  ): string {
    const icon = status === "completed" ? "✅" : status === "failed" ? "❌" : "⚪";
    const truncated = output.length > 4000 ? output.slice(0, 3997) + "..." : output;
    return [
      `## ${icon} Task ${status}`,
      "",
      truncated,
      "",
      costUsd !== undefined ? `**Cost**: $${costUsd.toFixed(2)}` : "",
    ]
      .filter(Boolean)
      .join("\n");
  }

  private issueToTask(issue: {
    number: number;
    title: string;
    body: string;
    state: string;
    labels: Array<{ name: string }>;
    createdAt: string;
    closedAt: string | null;
  }): Task | null {
    const labelNames = issue.labels.map((l) => l.name);

    // Must have orchestrator label
    if (!labelNames.includes("orchestrator")) return null;

    // Extract type from title or labels
    const titleMatch = issue.title.match(/^\[orchestrator:(\w[\w-]*)\]/);
    const typeLabel = labelNames.find((l) => l.startsWith("type:"));
    const type = (titleMatch?.[1] ?? typeLabel?.replace("type:", "") ?? "plan") as WorkflowType;

    // Extract priority from labels
    const priorityLabel = labelNames.find((l) => l.startsWith("priority:"));
    const priority = (priorityLabel?.replace("priority:", "") ?? "normal") as Priority;

    // Determine status
    let status: TaskStatus = "pending";
    if (issue.state === "CLOSED" || issue.state === "closed") {
      if (labelNames.includes("orchestrator:failed")) status = "failed";
      else if (labelNames.includes("orchestrator:cancelled")) status = "cancelled";
      else status = "completed";
    }

    // Parse input from body
    let input: TaskInput = {};
    try {
      const jsonMatch = issue.body.match(/```json\n([\s\S]*?)\n```/);
      if (jsonMatch?.[1]) {
        input = JSON.parse(jsonMatch[1]) as TaskInput;
      }
    } catch {
      // Body may not have JSON metadata
    }

    return {
      id: String(issue.number),
      type,
      status,
      priority,
      input,
      budget_usd: 10,
      cost_usd: 0,
      output: null,
      error: null,
      branch: null,
      pr_url: null,
      created_at: issue.createdAt,
      started_at: null,
      completed_at: issue.closedAt,
      session_id: null,
    };
  }

  private gh(args: string): string {
    // Args are built internally from validated constants (REPO, issue numbers)
    // not from user input, so shell injection risk is minimal.
    return execSync(`gh ${args}`, {
      encoding: "utf-8",
      timeout: 30_000,
      env: { ...process.env, GH_TOKEN: this.ghToken },
    }).trim();
  }

  private escape(str: string): string {
    return str.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
  }
}
