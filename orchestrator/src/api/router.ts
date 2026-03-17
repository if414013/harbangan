import { Router } from "express";
import type { Request, Response } from "express";
import { createChildLogger } from "../util/logger.js";
import type { AgentRegistry } from "../agents/registry.js";
import type { ITaskStore } from "../store/types.js";
import type { TaskQueue } from "../queue/task-queue.js";
import type { Scheduler } from "../queue/scheduler.js";
import type { BudgetTracker } from "../safety/budget.js";
import type { WorkspaceManager } from "../workspace/manager.js";
import type { Config } from "../config.js";
import type { KnowledgeBase } from "../knowledge/knowledge-base.js";
import { dispatchTaskSchema, listTasksSchema, slackCommandSchema, knowledgeCreateSchema, knowledgeUpdateSchema, knowledgeSearchSchema, knowledgeListSchema } from "./validators.js";
import { handleGitHubWebhook } from "./github-webhook.js";

const log = createChildLogger("router");

export interface RouterDeps {
  config: Config;
  registry: AgentRegistry;
  store: ITaskStore;
  queue: TaskQueue;
  scheduler: Scheduler;
  budget: BudgetTracker;
  workspace: WorkspaceManager;
  knowledgeBase: KnowledgeBase | null;
  startTime: number;
}

export function createRouter(deps: RouterDeps): Router {
  const router = Router();
  const {
    config,
    registry,
    store,
    queue,
    scheduler,
    budget,
    workspace,
    knowledgeBase,
    startTime,
  } = deps;

  // --- Health ---
  router.get("/health", (_req: Request, res: Response) => {
    res.json({
      status: workspace.isReady ? "healthy" : "degraded",
      uptime_seconds: Math.floor((Date.now() - startTime) / 1000),
      workspace_ready: workspace.isReady,
      queue_size: queue.size,
      active_tasks: scheduler.activeCount,
      version: "1.0.0",
    });
  });

  // --- Tasks ---
  router.post("/tasks", (req: Request, res: Response) => {
    const parsed = dispatchTaskSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
      return;
    }

    const { type, priority, input } = parsed.data;
    const taskBudget = input.budget_usd ?? config.defaultBudgetUsd;

    // Check budget
    const budgetCheck = budget.checkTaskBudget(taskBudget);
    if (!budgetCheck.allowed) {
      res.status(429).json({ error: budgetCheck.reason });
      return;
    }

    // Check queue pause
    if (queue.isPaused) {
      res.status(503).json({ error: "Queue is paused" });
      return;
    }

    const task = store.createTask(type, input, priority, taskBudget);
    store.updateTaskStatus(task.id, "queued");
    queue.enqueue({ ...task, status: "queued" });

    log.info({ taskId: task.id, type, priority }, "Task dispatched");

    res.status(201).json({
      id: task.id,
      type: task.type,
      status: "queued",
      priority: task.priority,
      budget_usd: task.budget_usd,
      created_at: task.created_at,
    });
  });

  router.get("/tasks", (req: Request, res: Response) => {
    const parsed = listTasksSchema.safeParse(req.query);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid query", details: parsed.error.issues });
      return;
    }

    const tasks = store.listTasks(parsed.data);
    res.json({ tasks, count: tasks.length });
  });

  router.get("/tasks/:id", (req: Request, res: Response) => {
    const id = Array.isArray(req.params.id) ? req.params.id[0] : req.params.id;
    const task = store.getTask(id);
    if (!task) {
      res.status(404).json({ error: "Task not found" });
      return;
    }

    const audit = store.getAuditLog(task.id, 50);
    res.json({ ...task, audit_log: audit });
  });

  router.post("/tasks/:id/cancel", (req: Request, res: Response) => {
    const id = Array.isArray(req.params.id) ? req.params.id[0] : req.params.id;
    const task = store.getTask(id);
    if (!task) {
      res.status(404).json({ error: "Task not found" });
      return;
    }

    if (task.status === "completed" || task.status === "failed" || task.status === "cancelled") {
      res.status(400).json({ error: `Task already ${task.status}` });
      return;
    }

    const cancelled = scheduler.cancelTask(task.id);
    if (!cancelled) {
      store.updateTaskStatus(task.id, "cancelled");
    }

    log.info({ taskId: task.id }, "Task cancelled");
    res.json({ id: task.id, status: "cancelled" });
  });

  // --- Agents ---
  router.get("/agents", (_req: Request, res: Response) => {
    res.json({ agents: registry.listAll() });
  });

  // --- Stats ---
  router.get("/stats", (_req: Request, res: Response) => {
    const stats = store.getStats(
      config.dailyBudgetLimitUsd,
      config.monthlyBudgetLimitUsd,
    );
    res.json(stats);
  });

  // --- Admin ---
  router.post("/admin/pause", (_req: Request, res: Response) => {
    queue.pause();
    log.warn("Queue paused via admin API");
    res.json({ status: "paused" });
  });

  router.post("/admin/resume", (_req: Request, res: Response) => {
    queue.resume();
    log.info("Queue resumed via admin API");
    res.json({ status: "resumed" });
  });

  router.post("/admin/stop", (_req: Request, res: Response) => {
    const count = scheduler.cancelAll();
    log.warn({ count }, "Emergency stop triggered");
    res.json({ status: "stopped", cancelled_count: count });
  });

  // --- Slack Webhook ---
  router.post("/slack/webhook", (req: Request, res: Response) => {
    const parsed = slackCommandSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid Slack command" });
      return;
    }

    const { text, user_name, channel_id } = parsed.data;
    const parts = text.trim().split(/\s+/);
    const subcommand = parts[0]?.toLowerCase();
    const args = parts.slice(1).join(" ");

    log.info({ user: user_name, command: subcommand, args, channel: channel_id }, "Slack command");

    // Acknowledge immediately (Slack requires <3s response)
    res.json({
      response_type: "ephemeral",
      text: `Processing: \`${subcommand} ${args}\`...`,
    });

    // Dispatch the actual work asynchronously
    handleSlackCommand(subcommand, args, channel_id, store, queue, config, scheduler).catch(
      (err) => log.error({ error: String(err) }, "Slack command handler failed"),
    );
  });

  // --- GitHub Webhook (no auth — uses signature verification) ---
  router.post("/github/webhook", handleGitHubWebhook(store, queue, config));

  // --- Knowledge Base ---
  router.get("/knowledge", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const parsed = knowledgeListSchema.safeParse(req.query);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid query", details: parsed.error.issues });
      return;
    }
    try {
      const entries = await knowledgeBase.list(parsed.data);
      res.json({ entries, count: entries.length });
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.get("/knowledge/stats", async (_req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    try {
      const stats = await knowledgeBase.stats();
      res.json(stats);
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.get("/knowledge/:id", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const id = Array.isArray(req.params.id) ? req.params.id[0] : req.params.id;
    try {
      const entry = await knowledgeBase.get(id);
      if (!entry) {
        res.status(404).json({ error: "Knowledge entry not found" });
        return;
      }
      res.json(entry);
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.post("/knowledge", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const parsed = knowledgeCreateSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
      return;
    }
    try {
      const entry = await knowledgeBase.add(parsed.data);
      res.status(201).json(entry);
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.put("/knowledge/:id", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const id = Array.isArray(req.params.id) ? req.params.id[0] : req.params.id;
    const parsed = knowledgeUpdateSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
      return;
    }
    try {
      const entry = await knowledgeBase.update(id, parsed.data);
      if (!entry) {
        res.status(404).json({ error: "Knowledge entry not found" });
        return;
      }
      res.json(entry);
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.delete("/knowledge/:id", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const id = Array.isArray(req.params.id) ? req.params.id[0] : req.params.id;
    try {
      const deleted = await knowledgeBase.delete(id);
      if (!deleted) {
        res.status(404).json({ error: "Knowledge entry not found" });
        return;
      }
      res.json({ deleted: true });
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  router.post("/knowledge/search", async (req: Request, res: Response) => {
    if (!knowledgeBase) {
      res.status(503).json({ error: "Knowledge base not configured" });
      return;
    }
    const parsed = knowledgeSearchSchema.safeParse(req.body);
    if (!parsed.success) {
      res.status(400).json({ error: "Invalid request", details: parsed.error.issues });
      return;
    }
    try {
      const results = await knowledgeBase.search(parsed.data.query, {
        type: parsed.data.type,
        limit: parsed.data.limit,
      });
      res.json({ results, count: results.length });
    } catch (err) {
      res.status(500).json({ error: String(err) });
    }
  });

  return router;
}

async function handleSlackCommand(
  command: string | undefined,
  args: string,
  channelId: string | undefined,
  store: ITaskStore,
  queue: TaskQueue,
  config: Config,
  _scheduler: Scheduler,
): Promise<void> {
  if (!command) return;

  switch (command) {
    case "plan": {
      const task = store.createTask("plan", { description: args, slack_channel_id: channelId }, "normal", config.defaultBudgetUsd);
      store.updateTaskStatus(task.id, "queued");
      queue.enqueue({ ...task, status: "queued" });
      break;
    }
    case "review": {
      const prNum = parseInt(args, 10);
      if (!isNaN(prNum)) {
        const task = store.createTask("pr-review", { pr_number: prNum, slack_channel_id: channelId }, "normal", config.defaultBudgetUsd);
        store.updateTaskStatus(task.id, "queued");
        queue.enqueue({ ...task, status: "queued" });
      }
      break;
    }
    case "implement": {
      const issueNum = parseInt(args, 10);
      if (!isNaN(issueNum)) {
        const task = store.createTask("implement", { issue_number: issueNum, slack_channel_id: channelId }, "normal", config.defaultBudgetUsd);
        store.updateTaskStatus(task.id, "queued");
        queue.enqueue({ ...task, status: "queued" });
      }
      break;
    }
    case "board":
    case "status": {
      const task = store.createTask("kanban", { command: args || "board-summary" }, "normal", 2);
      store.updateTaskStatus(task.id, "queued");
      queue.enqueue({ ...task, status: "queued" });
      break;
    }
    default:
      log.warn({ command }, "Unknown Slack command");
  }
}
