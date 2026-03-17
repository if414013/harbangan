import { describe, it, expect, beforeEach, vi } from "vitest";
import { GitHubTaskStore } from "../src/store/github-task-store.js";
import type { GitHubProjects } from "../src/store/github-projects.js";

// Mock child_process
vi.mock("node:child_process", () => ({
  execSync: vi.fn(),
}));

import { execSync } from "node:child_process";
const mockExec = vi.mocked(execSync);

function createMockProjects(): GitHubProjects {
  return {
    initialize: vi.fn(),
    addItemToProject: vi.fn().mockReturnValue("PVTI_item1"),
    updateTaskFields: vi.fn(),
    setSelectField: vi.fn(),
    setNumberField: vi.fn(),
    setTextField: vi.fn(),
    fieldIds: null,
  } as unknown as GitHubProjects;
}

describe("GitHubTaskStore", () => {
  let store: GitHubTaskStore;
  let mockProjects: GitHubProjects;

  beforeEach(() => {
    vi.clearAllMocks();
    mockProjects = createMockProjects();
    store = new GitHubTaskStore("fake-token", mockProjects);
  });

  describe("createTask", () => {
    it("should create a GitHub issue and return task with issue number as ID", () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ number: 42, nodeId: "I_abc123" })),
      );

      const task = store.createTask("plan", { description: "test plan" }, "normal", 10);

      expect(task.id).toBe("42");
      expect(task.type).toBe("plan");
      expect(task.status).toBe("pending");
      expect(task.priority).toBe("normal");
      expect(task.budget_usd).toBe(10);
      expect(task.input.description).toBe("test plan");
    });

    it("should fall back to URL parsing if JSON output fails", () => {
      mockExec
        .mockImplementationOnce(() => {
          throw new Error("gh json failed");
        })
        .mockReturnValueOnce(
          ("https://github.com/if414013/harbangan/issues/99"),
        );

      const task = store.createTask("implement", { issue_number: 5 });

      expect(task.id).toBe("99");
      expect(task.type).toBe("implement");
    });

    it("should fall back to local ID if all GitHub calls fail", () => {
      mockExec.mockImplementation(() => {
        throw new Error("gh failed");
      });

      const task = store.createTask("plan", { description: "offline" });

      expect(task.id).toMatch(/^local-/);
      expect(task.type).toBe("plan");
    });
  });

  describe("getTask / listTasks", () => {
    it("should return null for unknown task", () => {
      expect(store.getTask("nonexistent")).toBeNull();
    });

    it("should return created task from cache", () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ number: 1, nodeId: "I_1" })),
      );

      store.createTask("plan", { description: "cached" });
      const task = store.getTask("1");
      expect(task).not.toBeNull();
      expect(task!.input.description).toBe("cached");
    });

    it("should filter tasks by status", () => {
      mockExec
        .mockReturnValueOnce((JSON.stringify({ number: 1, nodeId: "I_1" })))
        .mockReturnValueOnce((JSON.stringify({ number: 2, nodeId: "I_2" })));

      store.createTask("plan", { description: "a" });
      store.createTask("plan", { description: "b" });

      const pending = store.listTasks({ status: "pending" });
      expect(pending).toHaveLength(2);

      const running = store.listTasks({ status: "running" });
      expect(running).toHaveLength(0);
    });

    it("should filter tasks by type", () => {
      mockExec
        .mockReturnValueOnce((JSON.stringify({ number: 1, nodeId: "I_1" })))
        .mockReturnValueOnce((JSON.stringify({ number: 2, nodeId: "I_2" })));

      store.createTask("plan", { description: "plan" });
      store.createTask("implement", { description: "impl" });

      const plans = store.listTasks({ type: "plan" });
      expect(plans).toHaveLength(1);
      expect(plans[0]!.type).toBe("plan");
    });

    it("should respect limit and offset", () => {
      for (let i = 1; i <= 5; i++) {
        mockExec.mockReturnValueOnce(
          (JSON.stringify({ number: i, nodeId: `I_${i}` })),
        );
        store.createTask("plan", { description: `task ${i}` });
      }

      const page = store.listTasks({ limit: 2, offset: 1 });
      expect(page).toHaveLength(2);
    });
  });

  describe("updateTaskStatus", () => {
    it("should update status in cache", () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ number: 1, nodeId: "I_1" })),
      );
      store.createTask("plan", { description: "test" });

      store.updateTaskStatus("1", "running");
      expect(store.getTask("1")!.status).toBe("running");
      expect(store.getTask("1")!.started_at).not.toBeNull();
    });

    it("should update extra fields", () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ number: 1, nodeId: "I_1" })),
      );
      store.createTask("plan", { description: "test" });

      store.updateTaskStatus("1", "completed", {
        output: "done",
        cost_usd: 5.0,
        branch: "feat/test",
        pr_url: "https://github.com/test/pr/1",
      });

      const task = store.getTask("1")!;
      expect(task.status).toBe("completed");
      expect(task.output).toBe("done");
      expect(task.cost_usd).toBe(5.0);
      expect(task.branch).toBe("feat/test");
      expect(task.pr_url).toBe("https://github.com/test/pr/1");
      expect(task.completed_at).not.toBeNull();
    });

    it("should not crash for unknown task", () => {
      store.updateTaskStatus("nonexistent", "running");
      // Should not throw
    });
  });

  describe("addAuditEntry / getAuditLog", () => {
    it("should buffer audit entries in memory", () => {
      store.addAuditEntry("task-1", "Read", "src/config.ts", true, 0);
      store.addAuditEntry("task-1", "Bash", "cargo test", true, 0.01);

      const log = store.getAuditLog("task-1");
      expect(log).toHaveLength(2);
      expect(log[0]!.tool_name).toBe("Read");
      expect(log[1]!.tool_name).toBe("Bash");
    });

    it("should return empty array for unknown task", () => {
      expect(store.getAuditLog("nonexistent")).toHaveLength(0);
    });

    it("should respect limit", () => {
      for (let i = 0; i < 10; i++) {
        store.addAuditEntry("task-1", `Tool${i}`, "input", true, 0);
      }

      expect(store.getAuditLog("task-1", 5)).toHaveLength(5);
    });
  });

  describe("budget tracking", () => {
    it("should calculate daily spend from cache", () => {
      mockExec
        .mockReturnValueOnce((JSON.stringify({ number: 1, nodeId: "I_1" })))
        .mockReturnValueOnce((JSON.stringify({ number: 2, nodeId: "I_2" })));

      store.createTask("plan", { description: "a" });
      store.createTask("plan", { description: "b" });

      store.updateTaskStatus("1", "completed", { cost_usd: 3.0 });
      store.updateTaskStatus("2", "running", { cost_usd: 2.0 });

      expect(store.getDailySpend()).toBe(5.0);
    });

    it("should return zero when no tasks exist", () => {
      expect(store.getDailySpend()).toBe(0);
      expect(store.getMonthlySpend()).toBe(0);
    });
  });

  describe("getStats", () => {
    it("should aggregate stats from cache", () => {
      mockExec
        .mockReturnValueOnce((JSON.stringify({ number: 1, nodeId: "I_1" })))
        .mockReturnValueOnce((JSON.stringify({ number: 2, nodeId: "I_2" })))
        .mockReturnValueOnce((JSON.stringify({ number: 3, nodeId: "I_3" })));

      store.createTask("plan", { description: "a" });
      store.createTask("plan", { description: "b" });
      store.createTask("plan", { description: "c" });

      store.updateTaskStatus("1", "completed", { cost_usd: 1.0 });
      store.updateTaskStatus("2", "failed", { error: "oops", cost_usd: 0.5 });

      const stats = store.getStats(100, 1000);
      expect(stats.total_tasks).toBe(3);
      expect(stats.completed_tasks).toBe(1);
      expect(stats.failed_tasks).toBe(1);
      expect(stats.total_cost_usd).toBe(1.5);
    });
  });

  describe("getActiveTasks / getQueuedTasks", () => {
    it("should return running tasks", () => {
      mockExec.mockReturnValue(
        (JSON.stringify({ number: 1, nodeId: "I_1" })),
      );
      store.createTask("plan", { description: "test" });
      store.updateTaskStatus("1", "running");

      expect(store.getActiveTasks()).toHaveLength(1);
      expect(store.getQueuedTasks()).toHaveLength(0);
    });
  });

  describe("initialize", () => {
    it("should load orchestrator-labeled issues into cache", async () => {
      mockExec.mockReturnValue(
        (
          JSON.stringify([
            {
              number: 10,
              title: "[orchestrator:plan] Test plan",
              body: "## Task Input\n```json\n{\"description\": \"loaded\"}\n```",
              state: "OPEN",
              labels: [{ name: "orchestrator" }, { name: "type:plan" }, { name: "priority:normal" }],
              createdAt: "2026-03-17T10:00:00Z",
              closedAt: null,
            },
            {
              number: 11,
              title: "[orchestrator:implement] Feature",
              body: "no json",
              state: "CLOSED",
              labels: [{ name: "orchestrator" }, { name: "type:implement" }],
              createdAt: "2026-03-16T10:00:00Z",
              closedAt: "2026-03-17T10:00:00Z",
            },
          ]),
        ),
      );

      await store.initialize();

      expect(store.getTask("10")).not.toBeNull();
      expect(store.getTask("10")!.type).toBe("plan");
      expect(store.getTask("10")!.input.description).toBe("loaded");

      expect(store.getTask("11")).not.toBeNull();
      expect(store.getTask("11")!.type).toBe("implement");
      expect(store.getTask("11")!.status).toBe("completed");
    });

    it("should handle empty issue list", async () => {
      mockExec.mockReturnValue(("[]"));
      await store.initialize();
      expect(store.listTasks()).toHaveLength(0);
    });
  });
});
