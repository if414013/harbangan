import { execSync } from "node:child_process";
import { createChildLogger } from "../util/logger.js";

const log = createChildLogger("github-tools");

const REPO = "if414013/harbangan";

/**
 * GitHub operations wrapper using gh CLI.
 * Used by workflows to interact with PRs, issues, and the project board.
 */
export class GitHubTools {
  private ghToken: string;

  constructor(ghToken: string) {
    this.ghToken = ghToken;
  }

  /** Get PR diff */
  getPrDiff(prNumber: number): string {
    return this.gh(`pr diff ${prNumber} --repo ${REPO}`);
  }

  /** Get PR metadata */
  getPrInfo(prNumber: number): string {
    return this.gh(
      `pr view ${prNumber} --repo ${REPO} --json title,body,state,author,files,reviews,comments`,
    );
  }

  /** List changed files in a PR */
  getPrFiles(prNumber: number): string {
    return this.gh(
      `pr view ${prNumber} --repo ${REPO} --json files --jq '.files[].path'`,
    );
  }

  /** Create a PR review comment */
  createPrReviewComment(
    prNumber: number,
    body: string,
    path: string,
    line: number,
  ): void {
    this.gh(
      `api repos/${REPO}/pulls/${prNumber}/comments -f body="${this.escape(body)}" -f path="${path}" -F line=${line} -f commit_id=$(gh pr view ${prNumber} --repo ${REPO} --json headRefOid --jq .headRefOid)`,
    );
    log.info({ prNumber, path, line }, "PR review comment created");
  }

  /** Submit a PR review */
  submitPrReview(
    prNumber: number,
    event: "APPROVE" | "REQUEST_CHANGES" | "COMMENT",
    body: string,
  ): void {
    this.gh(
      `pr review ${prNumber} --repo ${REPO} --${event.toLowerCase().replace("_", "-")} --body "${this.escape(body)}"`,
    );
    log.info({ prNumber, event }, "PR review submitted");
  }

  /** Create a GitHub issue */
  createIssue(title: string, body: string, labels: string[] = []): string {
    const labelArgs = labels.map((l) => `--label "${l}"`).join(" ");
    const result = this.gh(
      `issue create --repo ${REPO} --title "${this.escape(title)}" --body "${this.escape(body)}" ${labelArgs}`,
    );
    log.info({ title }, "Issue created");
    return result;
  }

  /** Close an issue */
  closeIssue(issueNumber: number, comment?: string): void {
    if (comment) {
      this.gh(
        `issue comment ${issueNumber} --repo ${REPO} --body "${this.escape(comment)}"`,
      );
    }
    this.gh(`issue close ${issueNumber} --repo ${REPO}`);
    log.info({ issueNumber }, "Issue closed");
  }

  /** Get issue details */
  getIssueDetails(issueNumber: number): string {
    return this.gh(
      `issue view ${issueNumber} --repo ${REPO} --json title,body,state,labels,assignees,comments`,
    );
  }

  /** List open issues */
  listOpenIssues(limit: number = 30): string {
    return this.gh(
      `issue list --repo ${REPO} --state open --limit ${limit} --json number,title,labels,assignees`,
    );
  }

  /** List open PRs */
  listOpenPrs(limit: number = 10): string {
    return this.gh(
      `pr list --repo ${REPO} --state open --limit ${limit} --json number,title,author,headRefName`,
    );
  }

  /** Create a PR */
  createPr(
    title: string,
    body: string,
    head: string,
    base: string = "main",
  ): string {
    const result = this.gh(
      `pr create --repo ${REPO} --title "${this.escape(title)}" --body "${this.escape(body)}" --head "${head}" --base "${base}"`,
    );
    log.info({ title, head, base }, "PR created");
    return result;
  }

  private gh(args: string): string {
    return execSync(`gh ${args}`, {
      encoding: "utf-8",
      timeout: 30_000,
      env: { ...process.env, GH_TOKEN: this.ghToken },
    }).trim();
  }

  private escape(str: string): string {
    return str.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n");
  }
}
