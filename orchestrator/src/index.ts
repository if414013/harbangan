import express from "express";
import { loadConfig } from "./config.js";
import { logger, createChildLogger } from "./util/logger.js";
import { AgentRegistry } from "./agents/registry.js";
import { SqliteTaskStore } from "./store/task-store.js";
import { GitHubTaskStore } from "./store/github-task-store.js";
import { GitHubProjects } from "./store/github-projects.js";
import type { ITaskStore } from "./store/types.js";
import { TaskQueue } from "./queue/task-queue.js";
import { Scheduler } from "./queue/scheduler.js";
import { WorkspaceManager } from "./workspace/manager.js";
import { WorktreeManager } from "./workspace/worktree.js";
import { SafetyGuardrails } from "./safety/guardrails.js";
import { BudgetTracker } from "./safety/budget.js";
import { AuditLogger } from "./safety/audit.js";
import { SlackNotifier } from "./util/slack.js";
import { SlackService } from "./slack/service.js";
import { InteractionBridge } from "./slack/interaction-bridge.js";
import { createRouter } from "./api/router.js";
import { authMiddleware, requestLogger, errorHandler } from "./api/middleware.js";
import { executePlanWorkflow } from "./workflows/plan.js";
import { executePrReviewWorkflow } from "./workflows/pr-review.js";
import { executeImplementWorkflow } from "./workflows/implement.js";
import { executeKanbanWorkflow } from "./workflows/kanban.js";
import { executeDocsWorkflow } from "./workflows/docs.js";
import { executeQaWorkflow } from "./workflows/qa.js";
import { KnowledgeBase } from "./knowledge/knowledge-base.js";
import { KnowledgeIngester } from "./knowledge/ingestion.js";
import { KnowledgeRetriever } from "./knowledge/retrieval.js";
import type { Task } from "./store/types.js";
import type { WorkflowContext } from "./workflows/base.js";

const log = createChildLogger("main");
const startTime = Date.now();

async function main() {
  // Load and validate config
  const config = loadConfig();
  log.info({ port: config.port, model: config.defaultModel }, "Configuration loaded");

  // Initialize core services
  const registry = new AgentRegistry();

  // Create task store based on config
  let store: ITaskStore;
  if (config.taskStoreBackend === "github") {
    if (!config.ghProjectNumber) {
      throw new Error("GH_PROJECT_NUMBER is required when TASK_STORE_BACKEND=github");
    }
    const projects = new GitHubProjects(
      config.ghToken,
      config.ghOwner,
      config.ghRepo,
      config.ghProjectNumber,
    );
    await projects.initialize();
    const ghStore = new GitHubTaskStore(config.ghToken, projects);
    await ghStore.initialize();
    store = ghStore;
    log.info({ projectNumber: config.ghProjectNumber }, "Using GitHub Projects task store");
  } else {
    store = new SqliteTaskStore(config.dataDir);
    log.info("Using SQLite task store");
  }

  const queue = new TaskQueue();
  const scheduler = new Scheduler(queue, store, config.maxConcurrentTasks);
  const guardrails = new SafetyGuardrails();
  const budget = new BudgetTracker(store, config.dailyBudgetLimitUsd, config.monthlyBudgetLimitUsd);
  const audit = new AuditLogger(store);
  const slack = new SlackNotifier(config.slackBotToken);

  // Initialize Slack interactive service (Socket Mode) if configured
  let slackService: SlackService | null = null;
  let interactionBridge: InteractionBridge | null = null;

  if (config.slackAppToken && config.slackBotToken) {
    interactionBridge = new InteractionBridge({
      askUserTimeoutMs: config.askUserTimeoutMs,
    });

    slackService = new SlackService(
      { appToken: config.slackAppToken, botToken: config.slackBotToken },
      interactionBridge,
    );

    // Wire up slash command handler
    slackService.onCommand = async (command, args, channelId, _userId) => {
      switch (command) {
        case "plan": {
          const task = store.createTask("plan", {
            description: args,
            slack_channel_id: channelId,
          }, "normal", config.defaultBudgetUsd);
          store.updateTaskStatus(task.id, "queued");
          queue.enqueue({ ...task, status: "queued" });
          return { text: `🔍 Planning task ${task.id.slice(0, 8)} created`, threadTs: "" };
        }
        case "review": {
          const prNum = parseInt(args, 10);
          if (isNaN(prNum)) return null;
          const task = store.createTask("pr-review", {
            pr_number: prNum,
            slack_channel_id: channelId,
          }, "normal", config.defaultBudgetUsd);
          store.updateTaskStatus(task.id, "queued");
          queue.enqueue({ ...task, status: "queued" });
          return { text: `📝 Review task ${task.id.slice(0, 8)} created`, threadTs: "" };
        }
        case "implement": {
          const issueNum = parseInt(args, 10);
          if (isNaN(issueNum)) return null;
          const task = store.createTask("implement", {
            issue_number: issueNum,
            slack_channel_id: channelId,
          }, "normal", config.defaultBudgetUsd);
          store.updateTaskStatus(task.id, "queued");
          queue.enqueue({ ...task, status: "queued" });
          return { text: `🔧 Implement task ${task.id.slice(0, 8)} created`, threadTs: "" };
        }
        case "board":
        case "status": {
          const task = store.createTask("kanban", {
            command: args || "board-summary",
            slack_channel_id: channelId,
          }, "normal", 2);
          store.updateTaskStatus(task.id, "queued");
          queue.enqueue({ ...task, status: "queued" });
          return { text: `📋 Board task created`, threadTs: "" };
        }
        default:
          return null;
      }
    };

    try {
      await slackService.start();
      log.info("Slack Socket Mode service started");
    } catch (err) {
      log.warn({ error: String(err) }, "Slack service failed to start — continuing without interactive Slack");
      slackService = null;
      interactionBridge = null;
    }
  }

  // Initialize workspace
  const workspace = new WorkspaceManager(
    config.workspaceDir,
    config.repoUrl,
    config.repoBranch,
    config.ghToken,
  );
  const worktrees = new WorktreeManager(workspace);

  // Initialize workspace asynchronously
  workspace.initialize().catch((err) => {
    log.error({ error: String(err) }, "Failed to initialize workspace");
  });

  // Initialize knowledge base if ChromaDB is configured
  let knowledgeBase: KnowledgeBase | null = null;
  let knowledgeRetriever: KnowledgeRetriever | null = null;
  let knowledgeIngester: KnowledgeIngester | null = null;

  if (config.chromaUrl) {
    knowledgeBase = new KnowledgeBase(config.chromaUrl);
    try {
      await knowledgeBase.initialize();
      knowledgeRetriever = new KnowledgeRetriever(knowledgeBase);
      knowledgeIngester = new KnowledgeIngester(knowledgeBase);
      log.info({ chromaUrl: config.chromaUrl }, "Knowledge base enabled");
    } catch (err) {
      knowledgeBase = null;
      log.warn({ error: String(err) }, "Knowledge base initialization failed — continuing without KB");
    }
  }

  // Set up task executor
  scheduler.setExecutor(async (task: Task) => {
    const ctx: WorkflowContext = {
      task,
      store,
      audit,
      guardrails,
      worktrees,
      repoPath: workspace.repoPath,
      knowledgeRetriever,
      interactionBridge,
      slackService,
    };

    let result;
    try {
      switch (task.type) {
        case "plan":
          result = await executePlanWorkflow(ctx, registry, store, queue);
          break;
        case "pr-review":
          result = await executePrReviewWorkflow(ctx, registry, config.ghToken, store, queue);
          break;
        case "implement":
          result = await executeImplementWorkflow(ctx, registry, config.ghToken);
          break;
        case "kanban":
          result = await executeKanbanWorkflow(ctx, registry);
          break;
        case "docs":
          result = await executeDocsWorkflow(ctx, registry);
          break;
        case "qa":
          result = await executeQaWorkflow(ctx, registry);
          break;
        default:
          result = {
            success: false,
            output: `Unknown workflow type: ${task.type}`,
            cost_usd: 0,
          };
      }

      store.updateTaskStatus(task.id, result.success ? "completed" : "failed", {
        output: result.output,
        cost_usd: result.cost_usd,
        branch: result.branch,
        pr_url: result.pr_url,
        error: result.success ? null : result.output,
      });

      // Notify via Slack if configured
      if (slack.isConfigured) {
        const msg = slack.formatTaskComplete(
          task.id,
          task.type,
          result.success,
          result.output,
          result.pr_url,
          result.cost_usd,
        );
        slack.notify("general", msg).catch(() => {});
      }

      // Auto-ingest into knowledge base
      if (knowledgeIngester) {
        const completedTask = store.getTask(task.id);
        if (completedTask) {
          knowledgeIngester.ingestTaskResult(completedTask).catch((err) => {
            log.warn({ taskId: task.id, error: String(err) }, "Knowledge ingestion failed");
          });
        }
      }
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      store.updateTaskStatus(task.id, "failed", { error: errMsg });
      log.error({ taskId: task.id, error: errMsg }, "Workflow execution failed");
    }
  });

  // Start scheduler
  scheduler.start();

  // Create Express app
  const app = express();
  app.use(express.json());
  app.use(express.urlencoded({ extended: true }));
  app.use(requestLogger());
  app.use(authMiddleware(config.orchestratorApiKey));

  const router = createRouter({
    config,
    registry,
    store,
    queue,
    scheduler,
    budget,
    workspace,
    startTime,
    knowledgeBase,
  });

  app.use("/api/v1", router);
  app.use(errorHandler());

  // Start server
  app.listen(config.port, () => {
    log.info(
      {
        port: config.port,
        agents: registry.size,
        maxConcurrent: config.maxConcurrentTasks,
        workspace: config.workspaceDir,
        slackEnabled: !!slackService,
        githubWebhookEnabled: !!config.githubWebhookSecret,
      },
      "Harbangan Orchestrator started",
    );
  });

  // Graceful shutdown
  const shutdown = async () => {
    log.info("Shutting down...");
    scheduler.stop();
    if (slackService) {
      await slackService.stop();
    }
    await Promise.resolve(store.close());
    process.exit(0);
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);

  // Periodic stale worktree cleanup (every hour)
  setInterval(
    () => {
      try {
        worktrees.cleanupStale();
      } catch (err) {
        log.warn({ error: String(err) }, "Stale worktree cleanup failed");
      }
    },
    60 * 60 * 1000,
  );
}

main().catch((err) => {
  logger.fatal({ error: String(err) }, "Fatal startup error");
  process.exit(1);
});
