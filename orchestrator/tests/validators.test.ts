import { describe, it, expect } from "vitest";
import { dispatchTaskSchema, listTasksSchema } from "../src/api/validators.js";

describe("dispatchTaskSchema", () => {
  it("should validate a plan task", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "plan",
      input: { description: "add rate limiting" },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.type).toBe("plan");
      expect(result.data.priority).toBe("normal");
    }
  });

  it("should validate a pr-review task", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "pr-review",
      input: { pr_number: 125 },
    });
    expect(result.success).toBe(true);
  });

  it("should validate an implement task with budget", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "implement",
      priority: "high",
      input: { issue_number: 42, budget_usd: 15 },
    });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.priority).toBe("high");
      expect(result.data.input.budget_usd).toBe(15);
    }
  });

  it("should reject invalid type", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "invalid",
      input: {},
    });
    expect(result.success).toBe(false);
  });

  it("should reject missing input", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "plan",
    });
    expect(result.success).toBe(false);
  });

  it("should reject negative PR number", () => {
    const result = dispatchTaskSchema.safeParse({
      type: "pr-review",
      input: { pr_number: -1 },
    });
    expect(result.success).toBe(false);
  });
});

describe("listTasksSchema", () => {
  it("should use defaults", () => {
    const result = listTasksSchema.safeParse({});
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.limit).toBe(50);
      expect(result.data.offset).toBe(0);
    }
  });

  it("should parse status filter", () => {
    const result = listTasksSchema.safeParse({ status: "running" });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.status).toBe("running");
    }
  });

  it("should reject invalid status", () => {
    const result = listTasksSchema.safeParse({ status: "invalid" });
    expect(result.success).toBe(false);
  });

  it("should coerce string limit", () => {
    const result = listTasksSchema.safeParse({ limit: "25" });
    expect(result.success).toBe(true);
    if (result.success) {
      expect(result.data.limit).toBe(25);
    }
  });

  it("should reject limit > 100", () => {
    const result = listTasksSchema.safeParse({ limit: 200 });
    expect(result.success).toBe(false);
  });
});
