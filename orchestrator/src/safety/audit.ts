import { createChildLogger } from "../util/logger.js";
import type { ITaskStore } from "../store/types.js";

const log = createChildLogger("audit");

export class AuditLogger {
  private store: ITaskStore;

  constructor(store: ITaskStore) {
    this.store = store;
  }

  logToolUse(
    taskId: string,
    toolName: string,
    toolInput: string,
    allowed: boolean,
    costUsd: number = 0,
  ): void {
    // Truncate input for storage
    const summary =
      toolInput.length > 500 ? toolInput.slice(0, 497) + "..." : toolInput;

    this.store.addAuditEntry(taskId, toolName, summary, allowed, costUsd);

    if (!allowed) {
      log.warn({ taskId, toolName, summary }, "Tool use denied");
    } else {
      log.debug({ taskId, toolName }, "Tool use logged");
    }
  }

  getLog(taskId: string, limit: number = 100) {
    return this.store.getAuditLog(taskId, limit);
  }
}
