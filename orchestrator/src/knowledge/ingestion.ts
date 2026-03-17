/**
 * Auto-indexes task outputs after completion.
 * Resolves knowledge type, generates title, and extracts tags from task results.
 */

import { createChildLogger } from "../util/logger.js";
import type { KnowledgeBase } from "./knowledge-base.js";
import type { KnowledgeType } from "./types.js";
import type { Task } from "../store/types.js";

const log = createChildLogger("knowledge:ingestion");

const MAX_CONTENT_LENGTH = 3000;

export class KnowledgeIngester {
  constructor(private kb: KnowledgeBase) {}

  /**
   * Ingest a completed task's output into the knowledge base.
   * Skips tasks with no meaningful output or kanban tasks.
   */
  async ingestTaskResult(task: Task): Promise<void> {
    if (!task.output || task.type === "kanban") {
      return;
    }

    const type = this.resolveType(task);
    const title = this.generateTitle(task);
    const tags = this.extractTags(task);
    const content = this.truncateContent(task.output);

    try {
      await this.kb.add({
        type,
        title,
        content,
        tags,
        source_task_id: task.id,
      });
      log.info(
        { taskId: task.id, type, title },
        "Task result ingested into knowledge base",
      );
    } catch (err) {
      log.error(
        { taskId: task.id, error: String(err) },
        "Failed to ingest task result",
      );
    }
  }

  /**
   * Resolve the knowledge type based on task type and status.
   */
  private resolveType(task: Task): KnowledgeType {
    if (task.status === "failed") {
      return "incident";
    }

    switch (task.type) {
      case "plan":
        return "decision";
      case "implement":
      case "docs":
      case "qa":
        return "task_summary";
      case "pr-review":
        return "learning";
      default:
        return "task_summary";
    }
  }

  /**
   * Generate a descriptive title from the task metadata.
   */
  private generateTitle(task: Task): string {
    const prefix = task.status === "failed" ? "[FAILED] " : "";
    const input = task.input;

    if (input.description) {
      // Truncate long descriptions to first sentence or 80 chars
      const firstSentence = input.description.split(/[.!?\n]/)[0] ?? input.description;
      return `${prefix}${task.type}: ${firstSentence.slice(0, 80)}`;
    }

    if (input.issue_number) {
      return `${prefix}${task.type}: issue #${input.issue_number}`;
    }

    if (input.pr_number) {
      return `${prefix}${task.type}: PR #${input.pr_number}`;
    }

    if (input.scope) {
      return `${prefix}${task.type}: ${input.scope}`;
    }

    return `${prefix}${task.type}: task ${task.id.slice(0, 8)}`;
  }

  /**
   * Extract tags from task metadata for filtering.
   */
  private extractTags(task: Task): string[] {
    const tags: string[] = [task.type];

    if (task.status === "failed") {
      tags.push("failed");
    }

    if (task.input.scope) {
      tags.push(task.input.scope);
    }

    if (task.branch) {
      tags.push("has-branch");
    }

    if (task.pr_url) {
      tags.push("has-pr");
    }

    return tags;
  }

  /**
   * Truncate content to a reasonable size for storage.
   */
  private truncateContent(output: string): string {
    if (output.length <= MAX_CONTENT_LENGTH) {
      return output;
    }
    return output.slice(0, MAX_CONTENT_LENGTH) + "\n\n[truncated]";
  }
}
