import { describe, it, expect, vi, beforeEach } from "vitest";
import { KnowledgeIngester } from "../src/knowledge/ingestion.js";
import { KnowledgeRetriever } from "../src/knowledge/retrieval.js";
import type { KnowledgeBase } from "../src/knowledge/knowledge-base.js";
import type {
  KnowledgeCreateInput,
  KnowledgeEntry,
  KnowledgeSearchResult,
  KnowledgeType,
} from "../src/knowledge/types.js";
import type { Task } from "../src/store/types.js";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function createMockTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "test-uuid-1234-5678-abcd-ef0123456789",
    type: "plan",
    status: "completed",
    priority: "normal",
    input: { description: "Add rate limiting to proxy endpoints" },
    budget_usd: 10,
    cost_usd: 2.5,
    output: "Implementation plan for rate limiting...",
    error: null,
    branch: null,
    pr_url: null,
    created_at: new Date().toISOString(),
    started_at: new Date().toISOString(),
    completed_at: new Date().toISOString(),
    session_id: null,
    ...overrides,
  };
}

function createMockKB() {
  const addCalls: KnowledgeCreateInput[] = [];
  const mock = {
    add: vi.fn(async (input: KnowledgeCreateInput): Promise<KnowledgeEntry> => {
      addCalls.push(input);
      return {
        id: "generated-id",
        type: input.type,
        title: input.title,
        content: input.content,
        tags: input.tags ?? [],
        source_task_id: input.source_task_id ?? null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      };
    }),
    searchByTypes: vi.fn(async (): Promise<KnowledgeSearchResult[]> => []),
    // Expose captured calls for assertions
    _addCalls: addCalls,
  };
  return mock as unknown as KnowledgeBase & { _addCalls: KnowledgeCreateInput[]; add: ReturnType<typeof vi.fn>; searchByTypes: ReturnType<typeof vi.fn> };
}

function makeEntry(overrides: Partial<KnowledgeEntry> = {}): KnowledgeEntry {
  return {
    id: "entry-1",
    type: "decision",
    title: "Rate limiting decision",
    content: "We decided to use token bucket algorithm for rate limiting.",
    tags: ["plan", "backend"],
    source_task_id: "task-1",
    created_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

// ===========================================================================
// Unit Tests — KnowledgeIngester
// ===========================================================================

describe("KnowledgeIngester", () => {
  let mockKB: ReturnType<typeof createMockKB>;
  let ingester: KnowledgeIngester;

  beforeEach(() => {
    mockKB = createMockKB();
    ingester = new KnowledgeIngester(mockKB);
  });

  // -------------------------------------------------------------------------
  // resolveType (tested through ingestTaskResult)
  // -------------------------------------------------------------------------

  describe("resolveType", () => {
    it("should resolve completed plan tasks as 'decision'", async () => {
      const task = createMockTask({ type: "plan", status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls).toHaveLength(1);
      expect(mockKB._addCalls[0].type).toBe("decision");
    });

    it("should resolve failed tasks as 'incident' regardless of type", async () => {
      const task = createMockTask({ type: "plan", status: "failed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("incident");
    });

    it("should resolve failed implement tasks as 'incident'", async () => {
      const task = createMockTask({ type: "implement", status: "failed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("incident");
    });

    it("should resolve implement tasks as 'task_summary'", async () => {
      const task = createMockTask({ type: "implement", status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("task_summary");
    });

    it("should resolve docs tasks as 'task_summary'", async () => {
      const task = createMockTask({ type: "docs", status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("task_summary");
    });

    it("should resolve qa tasks as 'task_summary'", async () => {
      const task = createMockTask({ type: "qa", status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("task_summary");
    });

    it("should resolve pr-review tasks as 'learning'", async () => {
      const task = createMockTask({ type: "pr-review", status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].type).toBe("learning");
    });
  });

  // -------------------------------------------------------------------------
  // generateTitle (tested through ingestTaskResult)
  // -------------------------------------------------------------------------

  describe("generateTitle", () => {
    it("should use description when available", async () => {
      const task = createMockTask({
        input: { description: "Add rate limiting to proxy endpoints" },
      });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe(
        "plan: Add rate limiting to proxy endpoints",
      );
    });

    it("should truncate description to first sentence", async () => {
      const task = createMockTask({
        input: {
          description:
            "Add rate limiting. This involves implementing token bucket algorithm and integration tests.",
        },
      });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("plan: Add rate limiting");
    });

    it("should truncate long descriptions to 80 chars", async () => {
      const longDesc = "A".repeat(120);
      const task = createMockTask({ input: { description: longDesc } });
      await ingester.ingestTaskResult(task);

      // "plan: " prefix + 80 chars of content
      const title = mockKB._addCalls[0].title;
      expect(title).toBe(`plan: ${"A".repeat(80)}`);
    });

    it("should prefix failed tasks with [FAILED]", async () => {
      const task = createMockTask({
        status: "failed",
        input: { description: "Deploy update" },
      });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("[FAILED] plan: Deploy update");
    });

    it("should fall back to issue_number", async () => {
      const task = createMockTask({ input: { issue_number: 42 } });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("plan: issue #42");
    });

    it("should fall back to pr_number", async () => {
      const task = createMockTask({ input: { pr_number: 99 } });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("plan: PR #99");
    });

    it("should fall back to scope", async () => {
      const task = createMockTask({ input: { scope: "backend" } });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("plan: backend");
    });

    it("should fall back to task id prefix when no metadata", async () => {
      const task = createMockTask({ input: {} });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].title).toBe("plan: task test-uui");
    });
  });

  // -------------------------------------------------------------------------
  // extractTags (tested through ingestTaskResult)
  // -------------------------------------------------------------------------

  describe("extractTags", () => {
    it("should always include task type as first tag", async () => {
      const task = createMockTask({ type: "implement" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).toContain("implement");
      expect(mockKB._addCalls[0].tags![0]).toBe("implement");
    });

    it("should add 'failed' tag for failed tasks", async () => {
      const task = createMockTask({ status: "failed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).toContain("failed");
    });

    it("should not add 'failed' tag for completed tasks", async () => {
      const task = createMockTask({ status: "completed" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).not.toContain("failed");
    });

    it("should add scope as a tag", async () => {
      const task = createMockTask({ input: { description: "test", scope: "backend" } });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).toContain("backend");
    });

    it("should add 'has-branch' when task has a branch", async () => {
      const task = createMockTask({ branch: "feat/rate-limiting" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).toContain("has-branch");
    });

    it("should not add 'has-branch' when branch is null", async () => {
      const task = createMockTask({ branch: null });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).not.toContain("has-branch");
    });

    it("should add 'has-pr' when task has a pr_url", async () => {
      const task = createMockTask({
        pr_url: "https://github.com/test/repo/pull/42",
      });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).toContain("has-pr");
    });

    it("should not add 'has-pr' when pr_url is null", async () => {
      const task = createMockTask({ pr_url: null });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].tags).not.toContain("has-pr");
    });

    it("should combine all applicable tags", async () => {
      const task = createMockTask({
        type: "implement",
        status: "failed",
        input: { description: "test", scope: "frontend" },
        branch: "fix/broken-ui",
        pr_url: "https://github.com/test/repo/pull/5",
      });
      await ingester.ingestTaskResult(task);

      const tags = mockKB._addCalls[0].tags!;
      expect(tags).toEqual(["implement", "failed", "frontend", "has-branch", "has-pr"]);
    });
  });

  // -------------------------------------------------------------------------
  // truncateContent (tested through ingestTaskResult)
  // -------------------------------------------------------------------------

  describe("truncateContent", () => {
    it("should pass through short content unchanged", async () => {
      const task = createMockTask({ output: "Short output" });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].content).toBe("Short output");
    });

    it("should truncate content over 3000 chars", async () => {
      const longContent = "x".repeat(4000);
      const task = createMockTask({ output: longContent });
      await ingester.ingestTaskResult(task);

      const content = mockKB._addCalls[0].content;
      expect(content.length).toBe(3000 + "\n\n[truncated]".length);
      expect(content.endsWith("\n\n[truncated]")).toBe(true);
      expect(content.startsWith("x".repeat(3000))).toBe(true);
    });

    it("should not truncate content at exactly 3000 chars", async () => {
      const exactContent = "y".repeat(3000);
      const task = createMockTask({ output: exactContent });
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].content).toBe(exactContent);
    });
  });

  // -------------------------------------------------------------------------
  // Skip conditions
  // -------------------------------------------------------------------------

  describe("ingestTaskResult skip conditions", () => {
    it("should skip tasks with no output", async () => {
      const task = createMockTask({ output: null });
      await ingester.ingestTaskResult(task);

      expect(mockKB.add).not.toHaveBeenCalled();
    });

    it("should skip kanban tasks", async () => {
      const task = createMockTask({ type: "kanban" });
      await ingester.ingestTaskResult(task);

      expect(mockKB.add).not.toHaveBeenCalled();
    });

    it("should pass source_task_id to KB add call", async () => {
      const task = createMockTask();
      await ingester.ingestTaskResult(task);

      expect(mockKB._addCalls[0].source_task_id).toBe(task.id);
    });

    it("should handle KB add failure gracefully", async () => {
      mockKB.add.mockRejectedValueOnce(new Error("ChromaDB unavailable"));
      const task = createMockTask();

      // Should not throw
      await expect(ingester.ingestTaskResult(task)).resolves.toBeUndefined();
    });
  });
});

// ===========================================================================
// Unit Tests — KnowledgeRetriever
// ===========================================================================

describe("KnowledgeRetriever", () => {
  let mockKB: ReturnType<typeof createMockKB>;
  let retriever: KnowledgeRetriever;

  beforeEach(() => {
    mockKB = createMockKB();
    retriever = new KnowledgeRetriever(mockKB);
  });

  describe("getContext", () => {
    it("should return empty context when no results", async () => {
      mockKB.searchByTypes.mockResolvedValueOnce([]);

      const ctx = await retriever.getContext("rate limiting", ["decision"]);

      expect(ctx.entries).toEqual([]);
      expect(ctx.formatted).toBe("");
    });

    it("should return formatted context with results", async () => {
      const results: KnowledgeSearchResult[] = [
        {
          entry: makeEntry({
            title: "Rate limiting decision",
            type: "decision",
            content: "We decided to use token bucket algorithm.",
            tags: ["plan", "backend"],
          }),
          score: 0.85,
        },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("rate limiting", ["decision"]);

      expect(ctx.entries).toHaveLength(1);
      expect(ctx.entries[0].score).toBe(0.85);
    });

    it("should include 'Relevant Project Knowledge' header in formatted output", async () => {
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry(), score: 0.9 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test query", ["decision"]);

      expect(ctx.formatted).toContain("## Relevant Project Knowledge");
    });

    it("should format entry with title, type, score, and tags", async () => {
      const results: KnowledgeSearchResult[] = [
        {
          entry: makeEntry({
            title: "Auth refactor",
            type: "learning",
            tags: ["pr-review", "auth"],
          }),
          score: 0.73,
        },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("auth", ["learning"]);

      expect(ctx.formatted).toContain("### 1. Auth refactor");
      expect(ctx.formatted).toContain("[pr-review, auth]");
      expect(ctx.formatted).toContain("**Type**: learning");
      expect(ctx.formatted).toContain("**Relevance**: 73%");
    });

    it("should number multiple entries correctly", async () => {
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry({ title: "First entry" }), score: 0.9 },
        { entry: makeEntry({ id: "entry-2", title: "Second entry" }), score: 0.7 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision", "learning"]);

      expect(ctx.formatted).toContain("### 1. First entry");
      expect(ctx.formatted).toContain("### 2. Second entry");
    });

    it("should truncate entries over 1500 chars", async () => {
      const longContent = "z".repeat(2000);
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry({ content: longContent }), score: 0.8 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision"]);

      // Content should be sliced to 1500 + "..."
      expect(ctx.formatted).toContain("z".repeat(1500) + "...");
      expect(ctx.formatted).not.toContain("z".repeat(1501));
    });

    it("should not truncate entries at or under 1500 chars", async () => {
      const exactContent = "z".repeat(1500);
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry({ content: exactContent }), score: 0.8 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision"]);

      expect(ctx.formatted).toContain(exactContent);
      expect(ctx.formatted).not.toContain(exactContent + "...");
    });

    it("should omit tags bracket when entry has no tags", async () => {
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry({ tags: [], title: "No tags entry" }), score: 0.5 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision"]);

      expect(ctx.formatted).toContain("### 1. No tags entry\n");
      expect(ctx.formatted).not.toContain("[]");
    });

    it("should separate entries with horizontal rules", async () => {
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry({ title: "A" }), score: 0.9 },
        { entry: makeEntry({ id: "e2", title: "B" }), score: 0.8 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision"]);

      expect(ctx.formatted).toContain("---");
    });

    it("should return empty context when KB throws an error", async () => {
      mockKB.searchByTypes.mockRejectedValueOnce(new Error("Connection refused"));

      const ctx = await retriever.getContext("test", ["decision"]);

      expect(ctx.entries).toEqual([]);
      expect(ctx.formatted).toBe("");
    });

    it("should pass types and limit to searchByTypes", async () => {
      mockKB.searchByTypes.mockResolvedValueOnce([]);

      await retriever.getContext("my query", ["decision", "incident"]);

      expect(mockKB.searchByTypes).toHaveBeenCalledWith(
        "my query",
        ["decision", "incident"],
        5, // MAX_RESULTS
      );
    });

    it("should round score percentage to nearest integer", async () => {
      const results: KnowledgeSearchResult[] = [
        { entry: makeEntry(), score: 0.666 },
      ];
      mockKB.searchByTypes.mockResolvedValueOnce(results);

      const ctx = await retriever.getContext("test", ["decision"]);

      // Math.round(0.666 * 100) = 67
      expect(ctx.formatted).toContain("**Relevance**: 67%");
    });
  });
});

// ===========================================================================
// Integration Tests — KnowledgeBase (requires ChromaDB)
// ===========================================================================

describe.skipIf(!process.env.CHROMA_URL)("KnowledgeBase integration", () => {
  // Dynamic import to avoid constructor errors when ChromaDB is not available
  let KnowledgeBaseClass: typeof import("../src/knowledge/knowledge-base.js").KnowledgeBase;
  let kb: InstanceType<typeof KnowledgeBaseClass>;

  beforeEach(async () => {
    const mod = await import("../src/knowledge/knowledge-base.js");
    KnowledgeBaseClass = mod.KnowledgeBase;
    kb = new KnowledgeBaseClass(process.env.CHROMA_URL!);
    await kb.initialize();
  });

  it("should add and get an entry", async () => {
    const entry = await kb.add({
      type: "decision",
      title: "Test decision",
      content: "We decided to use token bucket algorithm.",
      tags: ["test", "backend"],
      source_task_id: "task-integration-1",
    });

    expect(entry.id).toBeDefined();
    expect(entry.type).toBe("decision");
    expect(entry.title).toBe("Test decision");

    const retrieved = await kb.get(entry.id);
    expect(retrieved).not.toBeNull();
    expect(retrieved!.title).toBe("Test decision");
    expect(retrieved!.type).toBe("decision");
    expect(retrieved!.tags).toEqual(["test", "backend"]);

    // Cleanup
    await kb.delete(entry.id);
  });

  it("should update an entry", async () => {
    const entry = await kb.add({
      type: "learning",
      title: "Original title",
      content: "Original content",
    });

    const updated = await kb.update(entry.id, {
      title: "Updated title",
      content: "Updated content",
      type: "decision",
    });

    expect(updated).not.toBeNull();
    expect(updated!.title).toBe("Updated title");
    expect(updated!.content).toBe("Updated content");
    expect(updated!.type).toBe("decision");
    expect(updated!.updated_at).not.toBe(entry.updated_at);

    // Cleanup
    await kb.delete(entry.id);
  });

  it("should delete an entry", async () => {
    const entry = await kb.add({
      type: "incident",
      title: "To be deleted",
      content: "This will be removed",
    });

    const deleted = await kb.delete(entry.id);
    expect(deleted).toBe(true);

    const retrieved = await kb.get(entry.id);
    expect(retrieved).toBeNull();
  });

  it("should return false when deleting non-existent entry", async () => {
    const deleted = await kb.delete("non-existent-id");
    expect(deleted).toBe(false);
  });

  it("should list entries with type filter", async () => {
    const e1 = await kb.add({
      type: "decision",
      title: "Decision A",
      content: "Content A",
    });
    const e2 = await kb.add({
      type: "learning",
      title: "Learning B",
      content: "Content B",
    });

    const decisions = await kb.list({ type: "decision" });
    const decisionIds = decisions.map((e) => e.id);
    expect(decisionIds).toContain(e1.id);

    const learnings = await kb.list({ type: "learning" });
    const learningIds = learnings.map((e) => e.id);
    expect(learningIds).toContain(e2.id);

    // Cleanup
    await kb.delete(e1.id);
    await kb.delete(e2.id);
  });

  it("should search and return relevant results", async () => {
    const entry = await kb.add({
      type: "decision",
      title: "Rate limiting approach",
      content:
        "We will implement rate limiting using a token bucket algorithm with per-user quotas.",
      tags: ["backend", "rate-limiting"],
    });

    // Semantic search
    const results = await kb.search("token bucket rate limit");
    expect(results.length).toBeGreaterThan(0);
    expect(results[0].score).toBeGreaterThan(0);

    // Cleanup
    await kb.delete(entry.id);
  });

  it("should searchByTypes with type filter", async () => {
    const e1 = await kb.add({
      type: "decision",
      title: "Architecture decision",
      content: "We chose Axum over Actix for the web framework.",
    });
    const e2 = await kb.add({
      type: "incident",
      title: "Deployment failure",
      content: "The deployment to prod failed due to missing env vars.",
    });

    const decisionsOnly = await kb.searchByTypes("web framework choice", ["decision"]);
    const types = decisionsOnly.map((r) => r.entry.type);
    // All returned results should be "decision" type
    for (const t of types) {
      expect(t).toBe("decision");
    }

    // Cleanup
    await kb.delete(e1.id);
    await kb.delete(e2.id);
  });

  it("should return correct stats", async () => {
    const e1 = await kb.add({
      type: "decision",
      title: "Stats test A",
      content: "A",
    });
    const e2 = await kb.add({
      type: "decision",
      title: "Stats test B",
      content: "B",
    });
    const e3 = await kb.add({
      type: "incident",
      title: "Stats test C",
      content: "C",
    });

    const stats = await kb.stats();
    expect(stats.total).toBeGreaterThanOrEqual(3);
    expect(stats.by_type.decision).toBeGreaterThanOrEqual(2);
    expect(stats.by_type.incident).toBeGreaterThanOrEqual(1);

    // Cleanup
    await kb.delete(e1.id);
    await kb.delete(e2.id);
    await kb.delete(e3.id);
  });
});
