import { createChildLogger } from "../util/logger.js";
import type { ThreadBinding, PendingInteraction, InteractionBridgeOptions } from "./types.js";

const log = createChildLogger("interaction-bridge");

const DEFAULT_TIMEOUT_MS = 30 * 60 * 1000; // 30 minutes

/**
 * Central coordinator for Slack interactive workflows.
 *
 * Maps tasks to Slack threads and manages blocking Promises that resolve
 * when users reply in the thread. Multiple ask_user calls per task are
 * supported — replies resolve the oldest pending question (FIFO).
 */
export class InteractionBridge {
  private readonly bindings = new Map<string, ThreadBinding>();
  private readonly threadIndex = new Map<string, string>(); // threadTs → taskId
  private readonly timeoutMs: number;

  constructor(options?: Partial<InteractionBridgeOptions>) {
    this.timeoutMs = options?.askUserTimeoutMs ?? DEFAULT_TIMEOUT_MS;
  }

  /**
   * Register a task ↔ thread binding.
   * Called when a Slack command creates a task and posts the initial message.
   */
  registerThread(taskId: string, channelId: string, threadTs: string): void {
    this.bindings.set(taskId, {
      channelId,
      threadTs,
      taskId,
      pending: [],
    });
    this.threadIndex.set(threadTs, taskId);
    log.info({ taskId, channelId, threadTs }, "Thread registered");
  }

  /**
   * Ask the user a question in the task's Slack thread.
   * Returns a Promise that blocks until the user replies or timeout.
   *
   * The caller (ask_user MCP tool handler) is responsible for posting
   * the question message to Slack. This method only manages the Promise.
   */
  askUser(taskId: string, question: string): Promise<string> {
    const binding = this.bindings.get(taskId);
    if (!binding) {
      return Promise.reject(
        new Error(`No Slack thread registered for task ${taskId}`),
      );
    }

    return new Promise<string>((resolve, reject) => {
      const timer = setTimeout(() => {
        // Remove this interaction from the pending list
        const idx = binding.pending.findIndex((p) => p.timer === timer);
        if (idx >= 0) {
          binding.pending.splice(idx, 1);
        }
        reject(
          new Error(
            `Timed out waiting for user response after ${Math.round(this.timeoutMs / 60000)} minutes`,
          ),
        );
      }, this.timeoutMs);

      const interaction: PendingInteraction = {
        resolve,
        reject,
        timer,
        question,
        createdAt: Date.now(),
      };

      binding.pending.push(interaction);
      log.info(
        { taskId, pendingCount: binding.pending.length },
        "User interaction queued",
      );
    });
  }

  /**
   * Handle a user reply in a Slack thread.
   * Resolves the oldest pending Promise (FIFO) for the matching task.
   *
   * @returns true if a pending interaction was resolved
   */
  handleReply(threadTs: string, text: string): boolean {
    const taskId = this.threadIndex.get(threadTs);
    if (!taskId) {
      return false;
    }

    const binding = this.bindings.get(taskId);
    if (!binding || binding.pending.length === 0) {
      return false;
    }

    // Resolve oldest pending interaction (FIFO)
    const interaction = binding.pending.shift()!;
    clearTimeout(interaction.timer);
    interaction.resolve(text);

    log.info(
      { taskId, threadTs, remainingPending: binding.pending.length },
      "User reply resolved pending interaction",
    );

    return true;
  }

  /**
   * Get the thread binding for a task.
   */
  getBinding(taskId: string): ThreadBinding | undefined {
    return this.bindings.get(taskId);
  }

  /**
   * Check if a task has any pending interactions.
   */
  hasPending(taskId: string): boolean {
    const binding = this.bindings.get(taskId);
    return !!binding && binding.pending.length > 0;
  }

  /**
   * Clean up a task's binding when the task completes.
   * Rejects any remaining pending interactions.
   */
  cleanup(taskId: string): void {
    const binding = this.bindings.get(taskId);
    if (!binding) return;

    for (const interaction of binding.pending) {
      clearTimeout(interaction.timer);
      interaction.reject(new Error("Task completed, interaction cancelled"));
    }

    this.threadIndex.delete(binding.threadTs);
    this.bindings.delete(taskId);
    log.info({ taskId }, "Thread binding cleaned up");
  }

  /**
   * Number of active thread bindings.
   */
  get size(): number {
    return this.bindings.size;
  }
}
