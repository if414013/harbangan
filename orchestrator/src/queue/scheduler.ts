import { createChildLogger } from "../util/logger.js";
import type { ITaskStore, Task } from "../store/types.js";
import { TaskQueue } from "./task-queue.js";

const log = createChildLogger("scheduler");

export type TaskExecutor = (task: Task) => Promise<void>;

export class Scheduler {
  private queue: TaskQueue;
  private store: ITaskStore;
  private maxConcurrent: number;
  private activeTasks = new Map<string, AbortController>();
  private executor: TaskExecutor | null = null;
  private pollInterval: ReturnType<typeof setInterval> | null = null;
  private stopped = false;

  constructor(queue: TaskQueue, store: ITaskStore, maxConcurrent: number) {
    this.queue = queue;
    this.store = store;
    this.maxConcurrent = maxConcurrent;
  }

  setExecutor(executor: TaskExecutor): void {
    this.executor = executor;
  }

  start(): void {
    if (this.pollInterval) return;
    this.stopped = false;
    this.pollInterval = setInterval(() => this.tick(), 1000);
    log.info({ maxConcurrent: this.maxConcurrent }, "Scheduler started");
  }

  stop(): void {
    this.stopped = true;
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
    log.info("Scheduler stopped");
  }

  private tick(): void {
    if (this.stopped || this.queue.isPaused) return;
    if (!this.executor) return;

    while (this.activeTasks.size < this.maxConcurrent) {
      const task = this.queue.dequeue();
      if (!task) break;
      this.runTask(task);
    }
  }

  private runTask(task: Task): void {
    const controller = new AbortController();
    this.activeTasks.set(task.id, controller);

    this.store.updateTaskStatus(task.id, "running");

    log.info(
      { taskId: task.id, type: task.type, active: this.activeTasks.size },
      "Starting task execution",
    );

    this.executor!(task)
      .catch((err) => {
        log.error({ taskId: task.id, error: String(err) }, "Task execution failed");
        this.store.updateTaskStatus(task.id, "failed", {
          error: err instanceof Error ? err.message : String(err),
        });
      })
      .finally(() => {
        this.activeTasks.delete(task.id);
        log.info(
          { taskId: task.id, active: this.activeTasks.size },
          "Task execution finished",
        );
      });
  }

  cancelTask(taskId: string): boolean {
    // Try removing from queue first
    if (this.queue.remove(taskId)) {
      this.store.updateTaskStatus(taskId, "cancelled");
      return true;
    }

    // Try aborting active task
    const controller = this.activeTasks.get(taskId);
    if (controller) {
      controller.abort();
      this.activeTasks.delete(taskId);
      this.store.updateTaskStatus(taskId, "cancelled");
      log.info({ taskId }, "Active task cancelled");
      return true;
    }

    return false;
  }

  cancelAll(): number {
    let count = 0;

    // Cancel all active tasks
    for (const [taskId, controller] of this.activeTasks) {
      controller.abort();
      this.store.updateTaskStatus(taskId, "cancelled");
      count++;
    }
    this.activeTasks.clear();

    // Clear queue
    const queued = this.queue.list();
    for (const task of queued) {
      this.store.updateTaskStatus(task.id, "cancelled");
      count++;
    }
    this.queue.clear();

    log.warn({ count }, "All tasks cancelled");
    return count;
  }

  get activeCount(): number {
    return this.activeTasks.size;
  }

  get queueSize(): number {
    return this.queue.size;
  }

  isTaskActive(taskId: string): boolean {
    return this.activeTasks.has(taskId);
  }
}
