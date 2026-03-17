import { createChildLogger } from "../util/logger.js";
import type { Priority, Task } from "../store/types.js";

const log = createChildLogger("task-queue");

const PRIORITY_ORDER: Record<Priority, number> = {
  high: 0,
  normal: 1,
  low: 2,
};

export class TaskQueue {
  private queue: Task[] = [];
  private paused = false;

  enqueue(task: Task): void {
    this.queue.push(task);
    this.queue.sort(
      (a, b) => PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority],
    );
    log.info({ taskId: task.id, type: task.type, priority: task.priority }, "Task enqueued");
  }

  dequeue(): Task | undefined {
    if (this.paused) return undefined;
    const task = this.queue.shift();
    if (task) {
      log.info({ taskId: task.id }, "Task dequeued");
    }
    return task;
  }

  peek(): Task | undefined {
    return this.queue[0];
  }

  remove(taskId: string): boolean {
    const idx = this.queue.findIndex((t) => t.id === taskId);
    if (idx >= 0) {
      this.queue.splice(idx, 1);
      log.info({ taskId }, "Task removed from queue");
      return true;
    }
    return false;
  }

  get size(): number {
    return this.queue.length;
  }

  get isPaused(): boolean {
    return this.paused;
  }

  pause(): void {
    this.paused = true;
    log.warn("Queue paused");
  }

  resume(): void {
    this.paused = false;
    log.info("Queue resumed");
  }

  clear(): void {
    const count = this.queue.length;
    this.queue = [];
    log.warn({ count }, "Queue cleared");
  }

  list(): Task[] {
    return [...this.queue];
  }
}
