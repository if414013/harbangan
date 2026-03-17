import { createChildLogger } from "../util/logger.js";
import type { ITaskStore } from "../store/types.js";

const log = createChildLogger("budget");

export interface BudgetCheckResult {
  allowed: boolean;
  reason?: string;
}

export class BudgetTracker {
  private store: ITaskStore;
  private dailyLimit: number;
  private monthlyLimit: number;

  constructor(store: ITaskStore, dailyLimit: number, monthlyLimit: number) {
    this.store = store;
    this.dailyLimit = dailyLimit;
    this.monthlyLimit = monthlyLimit;
  }

  checkTaskBudget(taskBudgetUsd: number): BudgetCheckResult {
    const dailySpend = this.store.getDailySpend();
    const monthlySpend = this.store.getMonthlySpend();

    if (dailySpend + taskBudgetUsd > this.dailyLimit) {
      const reason = `Daily budget limit reached: $${dailySpend.toFixed(2)} spent of $${this.dailyLimit} limit`;
      log.warn({ dailySpend, dailyLimit: this.dailyLimit, taskBudget: taskBudgetUsd }, reason);
      return { allowed: false, reason };
    }

    if (monthlySpend + taskBudgetUsd > this.monthlyLimit) {
      const reason = `Monthly budget limit reached: $${monthlySpend.toFixed(2)} spent of $${this.monthlyLimit} limit`;
      log.warn({ monthlySpend, monthlyLimit: this.monthlyLimit, taskBudget: taskBudgetUsd }, reason);
      return { allowed: false, reason };
    }

    return { allowed: true };
  }

  recordCost(taskId: string, costUsd: number): void {
    this.store.updateTaskStatus(taskId, "running", { cost_usd: costUsd });
    log.debug({ taskId, cost: costUsd }, "Cost recorded");
  }

  getUsage() {
    return this.store.getBudgetUsage(this.dailyLimit, this.monthlyLimit);
  }
}
