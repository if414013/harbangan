import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("workspace");

export class WorkspaceManager {
  private repoDir: string;
  private repoUrl: string;
  private branch: string;
  private ghToken: string;
  private ready = false;

  constructor(
    workspaceDir: string,
    repoUrl: string,
    branch: string,
    ghToken: string,
  ) {
    this.repoDir = path.join(workspaceDir, "harbangan");
    this.repoUrl = repoUrl;
    this.branch = branch;
    this.ghToken = ghToken;
  }

  async initialize(): Promise<void> {
    this.configureGit();

    if (fs.existsSync(path.join(this.repoDir, ".git"))) {
      log.info({ repoDir: this.repoDir }, "Repository exists, pulling latest");
      this.exec(`git -C ${this.repoDir} fetch origin`);
      this.exec(
        `git -C ${this.repoDir} checkout ${this.branch} -- 2>/dev/null || true`,
      );
      this.exec(`git -C ${this.repoDir} pull origin ${this.branch} --ff-only`);
    } else {
      log.info(
        { repoDir: this.repoDir, repoUrl: this.repoUrl },
        "Cloning repository",
      );
      fs.mkdirSync(path.dirname(this.repoDir), { recursive: true });
      this.exec(
        `git clone --branch ${this.branch} ${this.repoUrl} ${this.repoDir}`,
      );
    }

    this.ready = true;
    log.info("Workspace initialized");
  }

  fetchLatest(): void {
    this.ensureReady();
    this.exec(`git -C ${this.repoDir} fetch origin`);
    log.debug("Fetched latest from origin");
  }

  get repoPath(): string {
    return this.repoDir;
  }

  get isReady(): boolean {
    return this.ready;
  }

  private configureGit(): void {
    // Configure gh CLI auth
    const env = { ...process.env, GH_TOKEN: this.ghToken };

    // Configure git to use token for HTTPS
    if (this.repoUrl.startsWith("https://")) {
      const tokenUrl = this.repoUrl.replace(
        "https://",
        `https://x-access-token:${this.ghToken}@`,
      );
      try {
        this.exec(
          `git config --global url."${tokenUrl}".insteadOf "${this.repoUrl}"`,
          env,
        );
      } catch {
        log.debug("Git URL rewrite config skipped");
      }
    }

    // Set basic git config for commits
    try {
      this.exec(
        'git config --global user.email "orchestrator@harbangan.dev"',
      );
      this.exec('git config --global user.name "Harbangan Orchestrator"');
    } catch {
      log.debug("Git user config already set");
    }
  }

  private ensureReady(): void {
    if (!this.ready) {
      throw new Error("Workspace not initialized. Call initialize() first.");
    }
  }

  private exec(
    cmd: string,
    env?: Record<string, string | undefined>,
  ): string {
    return execSync(cmd, {
      encoding: "utf-8",
      timeout: 120_000,
      env: env ?? process.env,
    }).trim();
  }
}
