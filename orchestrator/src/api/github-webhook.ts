import { createHmac, timingSafeEqual } from "node:crypto";
import type { Request, Response } from "express";
import { createChildLogger } from "../util/logger.js";
import type { ITaskStore } from "../store/types.js";
import type { TaskQueue } from "../queue/task-queue.js";
import type { Config } from "../config.js";

const log = createChildLogger("github-webhook");

interface PullRequestEvent {
  action: string;
  number: number;
  pull_request: {
    title: string;
    user: { login: string };
    head: { ref: string };
    base: { ref: string };
    html_url: string;
  };
}

/**
 * Verify GitHub webhook signature (HMAC-SHA256).
 */
function verifySignature(
  payload: string,
  signature: string | undefined,
  secret: string,
): boolean {
  if (!signature) return false;

  const expected = "sha256=" + createHmac("sha256", secret)
    .update(payload)
    .digest("hex");

  try {
    return timingSafeEqual(
      Buffer.from(signature),
      Buffer.from(expected),
    );
  } catch {
    return false;
  }
}

/**
 * Handle GitHub webhook POST requests.
 * Creates pr-review tasks for PR open/synchronize events.
 */
export function handleGitHubWebhook(
  store: ITaskStore,
  queue: TaskQueue,
  config: Config,
  slackChannelId?: string,
) {
  return (req: Request, res: Response): void => {
    if (!config.githubWebhookSecret) {
      res.status(503).json({ error: "GitHub webhook secret not configured" });
      return;
    }

    // Verify signature
    const signature = req.headers["x-hub-signature-256"] as string | undefined;
    const rawBody = JSON.stringify(req.body);

    if (!verifySignature(rawBody, signature, config.githubWebhookSecret)) {
      log.warn("GitHub webhook signature verification failed");
      res.status(401).json({ error: "Invalid signature" });
      return;
    }

    const event = req.headers["x-github-event"] as string;

    // Only handle pull_request events
    if (event !== "pull_request") {
      res.json({ status: "ignored", event });
      return;
    }

    const payload = req.body as PullRequestEvent;

    // Only trigger on opened or synchronized (new commits pushed)
    if (payload.action !== "opened" && payload.action !== "synchronize") {
      res.json({ status: "ignored", action: payload.action });
      return;
    }

    const prNumber = payload.number;
    const prTitle = payload.pull_request.title;
    const author = payload.pull_request.user.login;

    log.info(
      { prNumber, action: payload.action, author },
      "PR webhook received",
    );

    // Create a pr-review task
    const task = store.createTask(
      "pr-review",
      {
        pr_number: prNumber,
        description: `Review PR #${prNumber}: ${prTitle}`,
        slack_channel_id: slackChannelId,
      },
      "normal",
      config.defaultBudgetUsd,
    );
    store.updateTaskStatus(task.id, "queued");
    queue.enqueue({ ...task, status: "queued" });

    log.info({ taskId: task.id, prNumber }, "PR review task created from webhook");

    res.status(201).json({
      status: "review_queued",
      task_id: task.id,
      pr_number: prNumber,
    });
  };
}
