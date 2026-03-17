import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createChildLogger } from "../util/logger.js";
import type { WorkspaceManager } from "./manager.js";

const log = createChildLogger("worktree");

export interface WorktreeInfo {
  path: string;
  branch: string;
  taskId: string;
}

export class WorktreeManager {
  private workspace: WorkspaceManager;
  private activeWorktrees = new Map<string, WorktreeInfo>();

  constructor(workspace: WorkspaceManager) {
    this.workspace = workspace;
  }

  create(taskId: string, branchName: string): WorktreeInfo {
    this.workspace.fetchLatest();

    const treesDir = path.join(this.workspace.repoPath, ".trees");
    fs.mkdirSync(treesDir, { recursive: true });

    const worktreePath = path.join(treesDir, taskId);

    // Clean up if exists from a previous failed run
    if (fs.existsSync(worktreePath)) {
      log.warn({ taskId, path: worktreePath }, "Cleaning up stale worktree");
      this.remove(taskId);
    }

    this.git(this.workspace.repoPath, ["worktree", "add", worktreePath, "-b", branchName, "origin/main"]);

    const info: WorktreeInfo = {
      path: worktreePath,
      branch: branchName,
      taskId,
    };

    this.activeWorktrees.set(taskId, info);
    log.info({ taskId, branch: branchName, path: worktreePath }, "Worktree created");
    return info;
  }

  remove(taskId: string): void {
    const info = this.activeWorktrees.get(taskId);
    const treePath = info?.path ?? path.join(this.workspace.repoPath, ".trees", taskId);

    try {
      this.git(this.workspace.repoPath, ["worktree", "remove", "--force", treePath]);
    } catch {
      // Force cleanup if git worktree remove fails
      if (fs.existsSync(treePath)) {
        fs.rmSync(treePath, { recursive: true, force: true });
        try {
          this.git(this.workspace.repoPath, ["worktree", "prune"]);
        } catch {
          // ignore prune failures
        }
      }
    }

    this.activeWorktrees.delete(taskId);
    log.info({ taskId }, "Worktree removed");
  }

  get(taskId: string): WorktreeInfo | undefined {
    return this.activeWorktrees.get(taskId);
  }

  list(): WorktreeInfo[] {
    return [...this.activeWorktrees.values()];
  }

  pushBranch(taskId: string): void {
    const info = this.activeWorktrees.get(taskId);
    if (!info) throw new Error(`No worktree for task ${taskId}`);

    this.git(info.path, ["push", "-u", "origin", info.branch]);
    log.info({ taskId, branch: info.branch }, "Branch pushed");
  }

  commitAll(taskId: string, message: string): void {
    const info = this.activeWorktrees.get(taskId);
    if (!info) throw new Error(`No worktree for task ${taskId}`);

    this.git(info.path, ["add", "-A"]);

    // Check if there are changes to commit
    try {
      this.git(info.path, ["diff", "--cached", "--quiet"]);
      log.debug({ taskId }, "No changes to commit");
    } catch {
      // diff --quiet exits non-zero when there are changes
      this.git(info.path, ["commit", "-m", message]);
      log.info({ taskId, message }, "Changes committed");
    }
  }

  cleanupStale(maxAgeMs: number = 24 * 60 * 60 * 1000): void {
    const treesDir = path.join(this.workspace.repoPath, ".trees");
    if (!fs.existsSync(treesDir)) return;

    const now = Date.now();
    const entries = fs.readdirSync(treesDir, { withFileTypes: true });

    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const entryPath = path.join(treesDir, entry.name);
      const stat = fs.statSync(entryPath);

      if (now - stat.mtimeMs > maxAgeMs) {
        log.info({ path: entryPath }, "Removing stale worktree");
        this.remove(entry.name);
      }
    }
  }

  private git(cwd: string, args: string[]): string {
    return execFileSync("git", ["-C", cwd, ...args], {
      encoding: "utf-8",
      timeout: 60_000,
      env: process.env as NodeJS.ProcessEnv,
    }).trim();
  }
}
