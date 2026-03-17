import { describe, it, expect, beforeEach } from "vitest";
import { TaskQueue } from "../src/queue/task-queue.js";
import type { Task } from "../src/store/types.js";

function makeTask(id: string, priority: "high" | "normal" | "low" = "normal"): Task {
  return {
    id,
    type: "plan",
    status: "queued",
    priority,
    input: { description: "test" },
    budget_usd: 10,
    cost_usd: 0,
    output: null,
    error: null,
    branch: null,
    pr_url: null,
    created_at: new Date().toISOString(),
    started_at: null,
    completed_at: null,
    session_id: null,
  };
}

describe("TaskQueue", () => {
  let queue: TaskQueue;

  beforeEach(() => {
    queue = new TaskQueue();
  });

  it("should enqueue and dequeue tasks", () => {
    const task = makeTask("task-1");
    queue.enqueue(task);
    expect(queue.size).toBe(1);

    const dequeued = queue.dequeue();
    expect(dequeued?.id).toBe("task-1");
    expect(queue.size).toBe(0);
  });

  it("should return undefined when empty", () => {
    expect(queue.dequeue()).toBeUndefined();
  });

  it("should prioritize high > normal > low", () => {
    queue.enqueue(makeTask("low-1", "low"));
    queue.enqueue(makeTask("high-1", "high"));
    queue.enqueue(makeTask("normal-1", "normal"));

    expect(queue.dequeue()?.id).toBe("high-1");
    expect(queue.dequeue()?.id).toBe("normal-1");
    expect(queue.dequeue()?.id).toBe("low-1");
  });

  it("should peek without removing", () => {
    queue.enqueue(makeTask("task-1"));
    expect(queue.peek()?.id).toBe("task-1");
    expect(queue.size).toBe(1);
  });

  it("should remove specific task", () => {
    queue.enqueue(makeTask("task-1"));
    queue.enqueue(makeTask("task-2"));
    expect(queue.remove("task-1")).toBe(true);
    expect(queue.size).toBe(1);
    expect(queue.dequeue()?.id).toBe("task-2");
  });

  it("should return false when removing nonexistent task", () => {
    expect(queue.remove("nonexistent")).toBe(false);
  });

  it("should not dequeue when paused", () => {
    queue.enqueue(makeTask("task-1"));
    queue.pause();
    expect(queue.dequeue()).toBeUndefined();
    expect(queue.isPaused).toBe(true);
  });

  it("should dequeue after resume", () => {
    queue.enqueue(makeTask("task-1"));
    queue.pause();
    queue.resume();
    expect(queue.isPaused).toBe(false);
    expect(queue.dequeue()?.id).toBe("task-1");
  });

  it("should clear all tasks", () => {
    queue.enqueue(makeTask("task-1"));
    queue.enqueue(makeTask("task-2"));
    queue.clear();
    expect(queue.size).toBe(0);
  });

  it("should list queued tasks", () => {
    queue.enqueue(makeTask("task-1"));
    queue.enqueue(makeTask("task-2"));
    const list = queue.list();
    expect(list).toHaveLength(2);
    expect(list[0].id).toBe("task-1");
  });
});
