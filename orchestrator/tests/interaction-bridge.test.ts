import { describe, it, expect, vi, beforeEach } from "vitest";
import { InteractionBridge } from "../src/slack/interaction-bridge.js";

describe("InteractionBridge", () => {
  let bridge: InteractionBridge;

  beforeEach(() => {
    bridge = new InteractionBridge({ askUserTimeoutMs: 1000 }); // 1s timeout for tests
  });

  it("should register and retrieve a thread binding", () => {
    bridge.registerThread("task-1", "C123", "1234567890.123456");
    const binding = bridge.getBinding("task-1");
    expect(binding).toBeDefined();
    expect(binding!.channelId).toBe("C123");
    expect(binding!.threadTs).toBe("1234567890.123456");
    expect(binding!.taskId).toBe("task-1");
    expect(binding!.pending).toHaveLength(0);
  });

  it("should resolve pending interaction on reply", async () => {
    bridge.registerThread("task-1", "C123", "ts-1");

    const promise = bridge.askUser("task-1", "What color?");
    expect(bridge.hasPending("task-1")).toBe(true);

    const resolved = bridge.handleReply("ts-1", "blue");
    expect(resolved).toBe(true);

    const result = await promise;
    expect(result).toBe("blue");
    expect(bridge.hasPending("task-1")).toBe(false);
  });

  it("should resolve FIFO when multiple questions are pending", async () => {
    bridge.registerThread("task-1", "C123", "ts-1");

    const p1 = bridge.askUser("task-1", "Question 1?");
    const p2 = bridge.askUser("task-1", "Question 2?");

    bridge.handleReply("ts-1", "answer-1");
    bridge.handleReply("ts-1", "answer-2");

    expect(await p1).toBe("answer-1");
    expect(await p2).toBe("answer-2");
  });

  it("should reject on timeout", async () => {
    bridge = new InteractionBridge({ askUserTimeoutMs: 50 }); // 50ms
    bridge.registerThread("task-1", "C123", "ts-1");

    await expect(bridge.askUser("task-1", "Quick?")).rejects.toThrow("Timed out");
  });

  it("should reject when no thread is registered", async () => {
    await expect(bridge.askUser("no-task", "Hello?")).rejects.toThrow("No Slack thread registered");
  });

  it("should return false for reply to unknown thread", () => {
    expect(bridge.handleReply("unknown-ts", "hello")).toBe(false);
  });

  it("should return false for reply with no pending interactions", () => {
    bridge.registerThread("task-1", "C123", "ts-1");
    expect(bridge.handleReply("ts-1", "hello")).toBe(false);
  });

  it("should cleanup and reject pending interactions", async () => {
    bridge.registerThread("task-1", "C123", "ts-1");

    const promise = bridge.askUser("task-1", "Will this complete?");
    bridge.cleanup("task-1");

    await expect(promise).rejects.toThrow("Task completed");
    expect(bridge.getBinding("task-1")).toBeUndefined();
    expect(bridge.size).toBe(0);
  });

  it("should track size correctly", () => {
    expect(bridge.size).toBe(0);
    bridge.registerThread("task-1", "C1", "ts-1");
    bridge.registerThread("task-2", "C2", "ts-2");
    expect(bridge.size).toBe(2);
    bridge.cleanup("task-1");
    expect(bridge.size).toBe(1);
  });
});
