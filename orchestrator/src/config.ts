import { z } from "zod";

const configSchema = z.object({
  // Required
  anthropicApiKey: z.string().min(1),
  ghToken: z.string().min(1),
  orchestratorApiKey: z.string().min(1),
  repoUrl: z.string().min(1),

  // Optional with defaults
  repoBranch: z.string().default("main"),
  port: z.coerce.number().int().positive().default(3000),
  maxConcurrentTasks: z.coerce.number().int().positive().default(3),
  defaultBudgetUsd: z.coerce.number().positive().default(10),
  dailyBudgetLimitUsd: z.coerce.number().positive().default(100),
  monthlyBudgetLimitUsd: z.coerce.number().positive().default(1000),
  defaultModel: z.string().default("claude-sonnet-4-6"),

  // Slack (optional)
  slackBotToken: z.string().optional(),
  slackAppToken: z.string().optional(),
  slackSigningSecret: z.string().optional(),
  askUserTimeoutMs: z.coerce.number().int().positive().default(30 * 60 * 1000),

  // GitHub Webhooks (optional)
  githubWebhookSecret: z.string().optional(),

  // Task store backend
  taskStoreBackend: z.enum(["sqlite", "github"]).default("sqlite"),
  ghProjectNumber: z.coerce.number().int().positive().optional(),
  ghOwner: z.string().default("if414013"),
  ghRepo: z.string().default("harbangan"),

  // ChromaDB (optional)
  chromaUrl: z.string().url().optional(),

  // Paths
  workspaceDir: z.string().default("/workspace"),
  dataDir: z.string().default("/data"),
});

export type Config = z.infer<typeof configSchema>;

export function loadConfig(): Config {
  const raw = {
    anthropicApiKey: process.env.ANTHROPIC_API_KEY,
    ghToken: process.env.GH_TOKEN,
    orchestratorApiKey: process.env.ORCHESTRATOR_API_KEY,
    repoUrl: process.env.REPO_URL,
    repoBranch: process.env.REPO_BRANCH,
    port: process.env.ORCHESTRATOR_PORT,
    maxConcurrentTasks: process.env.MAX_CONCURRENT_TASKS,
    defaultBudgetUsd: process.env.DEFAULT_BUDGET_USD,
    dailyBudgetLimitUsd: process.env.DAILY_BUDGET_LIMIT_USD,
    monthlyBudgetLimitUsd: process.env.MONTHLY_BUDGET_LIMIT_USD,
    defaultModel: process.env.DEFAULT_MODEL,
    taskStoreBackend: process.env.TASK_STORE_BACKEND,
    ghProjectNumber: process.env.GH_PROJECT_NUMBER || undefined,
    ghOwner: process.env.GH_OWNER,
    ghRepo: process.env.GH_REPO,
    slackBotToken: process.env.SLACK_BOT_TOKEN || undefined,
    slackAppToken: process.env.SLACK_APP_TOKEN || undefined,
    slackSigningSecret: process.env.SLACK_SIGNING_SECRET || undefined,
    askUserTimeoutMs: process.env.ASK_USER_TIMEOUT_MS || undefined,
    githubWebhookSecret: process.env.GITHUB_WEBHOOK_SECRET || undefined,
    chromaUrl: process.env.CHROMA_URL || undefined,
    workspaceDir: process.env.WORKSPACE_DIR,
    dataDir: process.env.DATA_DIR,
  };

  const result = configSchema.safeParse(raw);
  if (!result.success) {
    const errors = result.error.issues
      .map((i) => `  ${i.path.join(".")}: ${i.message}`)
      .join("\n");
    throw new Error(`Invalid configuration:\n${errors}`);
  }

  return result.data;
}
