import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { loadConfig } from "../src/config.js";

describe("loadConfig", () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    // Set required env vars
    process.env.ANTHROPIC_API_KEY = "test-key";
    process.env.GH_TOKEN = "test-gh-token";
    process.env.ORCHESTRATOR_API_KEY = "test-api-key";
    process.env.REPO_URL = "https://github.com/test/repo.git";
  });

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it("should load config with required vars", () => {
    const config = loadConfig();
    expect(config.anthropicApiKey).toBe("test-key");
    expect(config.ghToken).toBe("test-gh-token");
    expect(config.orchestratorApiKey).toBe("test-api-key");
    expect(config.repoUrl).toBe("https://github.com/test/repo.git");
  });

  it("should use defaults for optional vars", () => {
    const config = loadConfig();
    expect(config.repoBranch).toBe("main");
    expect(config.port).toBe(3000);
    expect(config.maxConcurrentTasks).toBe(3);
    expect(config.defaultBudgetUsd).toBe(10);
    expect(config.dailyBudgetLimitUsd).toBe(100);
    expect(config.monthlyBudgetLimitUsd).toBe(1000);
    expect(config.defaultModel).toBe("claude-sonnet-4-6");
    expect(config.workspaceDir).toBe("/workspace");
    expect(config.dataDir).toBe("/data");
  });

  it("should override defaults with env vars", () => {
    process.env.ORCHESTRATOR_PORT = "4000";
    process.env.MAX_CONCURRENT_TASKS = "5";
    process.env.DEFAULT_BUDGET_USD = "20";

    const config = loadConfig();
    expect(config.port).toBe(4000);
    expect(config.maxConcurrentTasks).toBe(5);
    expect(config.defaultBudgetUsd).toBe(20);
  });

  it("should throw on missing required vars", () => {
    delete process.env.ANTHROPIC_API_KEY;
    expect(() => loadConfig()).toThrow("Invalid configuration");
  });

  it("should handle Slack config as optional", () => {
    const config = loadConfig();
    expect(config.slackBotToken).toBeUndefined();
    expect(config.slackSigningSecret).toBeUndefined();
  });

  it("should load Slack config when provided", () => {
    process.env.SLACK_BOT_TOKEN = "xoxb-test";
    process.env.SLACK_SIGNING_SECRET = "test-secret";

    const config = loadConfig();
    expect(config.slackBotToken).toBe("xoxb-test");
    expect(config.slackSigningSecret).toBe("test-secret");
  });
});
