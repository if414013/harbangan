# Research: Git Worktrees for Parallel Multi-Agent AI Development

## 1. Git Worktree Best Practices for Parallel Development

### What Are Git Worktrees?

Git worktrees (`git worktree add`) create additional working directories linked to the same repository. Each worktree gets its own directory with its own branch checked out, while sharing the same `.git` database, history, and remote connections. Unlike cloning a repository multiple times, worktrees are lightweight — they only create the working files needed.

### Core Best Practices

**Directory Organization:**
- Keep all worktrees under a dedicated directory (e.g., `.trees/` or `../repo-worktrees/`) to contain the project. Add the directory to `.gitignore`.
- Name worktree directories after the branch or feature for easy identification.

**One Branch Per Worktree Rule:**
- Git enforces that the same branch cannot be checked out in more than one worktree simultaneously. If you need the same code in two places, create a new branch.

**Shared History, Independent Files:**
- All worktrees share the same Git history — commits made in any worktree are immediately available to all others.
- Each directory is completely independent: its own files, running processes, and build artifacts.
- You can merge between worktrees using standard git commands.

**Environment Setup:**
- Worktrees do NOT inherit `.gitignore`-d files. That means `node_modules/`, `.env`, `dist/`, `.venv` do not exist in new worktrees. You must run dependency installation (e.g., `npm install`, `cargo build`) in each new worktree.
- Consider post-checkout hooks or setup scripts to automate dependency installation.

**Cleanup:**
- Remove worktrees when done: `git worktree remove <path>` or `git worktree prune` for stale entries.
- Commit or stash changes before removing a worktree — uncommitted changes in a deleted worktree are lost.

### When Worktrees Excel

| Use Case | Why Worktrees Help |
|----------|-------------------|
| Parallel feature development | Work on 2+ independent features without stashing/switching |
| Emergency hotfixes | Fix production bugs while keeping your feature branch untouched |
| Code review | Test a teammate's PR without disrupting your current work |
| Long-running processes | Run test suites in one worktree while coding in another |
| AI agent parallelism | Each agent gets its own isolated workspace |

---

## 2. Claude Code Worktree Support

### Built-in Worktree Support

Claude Code (v2.1.50+) has first-class support for git worktrees across CLI, Desktop app, and IDE extensions.

**Two ways to use worktrees:**

1. **Interactive (CLI/Desktop):** Ask Claude to "use worktrees for your agents" or use the `EnterWorktree` tool to create a new worktree session.

2. **Agent Definition (Subagents):** Add `isolation: worktree` to a custom agent's frontmatter:
   ```yaml
   ---
   isolation: worktree
   ---
   # Agent instructions here
   ```
   This creates a temporary worktree for the entire subagent session, isolating all file operations from the main working directory.

### Worktree Lifecycle in Claude Code

1. **Creation:** When `isolation: worktree` is specified (or `EnterWorktree` is called), Claude creates a new branch and worktree directory automatically.
2. **Execution:** The subagent works entirely within its worktree — all reads, writes, and git operations are scoped to that directory.
3. **Cleanup:**
   - **No changes made:** The worktree and its branch are removed automatically.
   - **Changes/commits exist:** Claude prompts whether to keep or remove the worktree. Keeping preserves the directory and branch for later use.
4. **Merge back:** Changes are merged back through standard git workflows — branches, pull requests, and code review. No special merge mechanism; it uses normal `git merge` or PR flow.

### Current Limitations

- `EnterWorktree` only creates **new** worktrees. There is no platform-level mechanism to re-enter an existing worktree in a new session.
- After the first session, the main benefit (platform-level directory tracking that survives context compaction) is lost for subsequent sessions.
- There is an open feature request (GitHub issue #31969) for entering/resuming existing worktrees, configurable branch naming, and hook removal control.

### Agent Teams

Claude Code's Agent Teams feature provides automated coordination of parallel sessions:
- One session acts as team lead, coordinating work.
- Teammates work independently in their own context windows.
- Each teammate can use worktree isolation.
- Communication happens through shared tasks and messaging.

---

## 3. Multi-Agent Parallel Development Patterns

### The Standard Pattern

```
Main repo (main branch)
├── .trees/feature-a/    ← Agent 1 worktree (feat/feature-a branch)
├── .trees/feature-b/    ← Agent 2 worktree (feat/feature-b branch)
├── .trees/bugfix-c/     ← Agent 3 worktree (fix/bugfix-c branch)
└── (main working dir)   ← Human developer or coordinator agent
```

Each agent:
1. Gets its own worktree with a dedicated feature branch
2. Works independently — reads, writes, commits, pushes
3. Opens a PR when complete
4. Worktree is cleaned up after PR merge

### Real-World Results: incident.io Case Study

incident.io reported transformative results:
- A task estimated at **2 hours** took **10 minutes** by running 5 parallel agents.
- Each agent was autonomous end-to-end: own branch, own directory, own ability to commit, push, and open a PR.
- They routinely run **4-5 Claude Code agents** simultaneously on different features.
- More time is spent on creative product development rather than fighting tooling.

### Tooling Ecosystem (2025-2026)

| Tool | Description |
|------|-------------|
| **ccswarm** | Multi-agent orchestration with specialized pools (Frontend, Backend, DevOps, QA) in worktree-isolated environments |
| **agent-worktree** | Git worktree workflow tool for AI coding agents with isolated environments |
| **Worktrunk** | CLI for Git worktree management designed for parallel AI agent workflows (inspired by incident.io) |
| **ccpm** | Project management using GitHub Issues + Git worktrees for parallel agent execution |
| **Clash** | Detects merge conflicts across worktrees *before* they happen |
| **agentree** | Quick worktree creation for AI workflows |
| **worktree-cli** | MCP server integration for Claude Code with automatic setup hooks |

### Scoping Work for Parallel Agents

The key to successful parallel agent work is **file ownership isolation**:
- Each agent should own a distinct set of files.
- Avoid two agents modifying the same file simultaneously.
- Structure tasks so that backend/frontend/tests/infra are worked on by different agents in different worktrees.
- Use a coordinator (human or lead agent) to decompose work into non-overlapping units.

### Known Gaps

- **No environment isolation beyond files:** Worktrees share the same local database, Docker daemon, and cache directories. Two agents modifying database state simultaneously creates race conditions.
- **Dependency installation overhead:** Each worktree needs its own `node_modules`, build artifacts, etc.
- **Agents are blind to each other:** Without tooling like Clash, agents don't know what other agents are changing.

---

## 4. Worktree vs. Branch Strategies

### When to Use Worktrees vs. Branches

| Scenario | Branches Alone | Worktrees |
|----------|---------------|-----------|
| Single developer, sequential work | Sufficient | Unnecessary overhead |
| Single developer, context switching | Stash/commit dance needed | Each branch ready to go in its own directory |
| Multiple AI agents in parallel | Impossible (filesystem conflicts) | Essential for isolation |
| CI/CD building multiple branches | Works (separate runners) | Useful for local multi-branch builds |
| Code review while developing | Requires stashing current work | Just `cd` to reviewer worktree |
| Long-running test suites | Blocks other work on same branch | Run tests in one worktree, code in another |

### Key Distinction

- **Branches** are a Git concept — they track divergent lines of development.
- **Worktrees** are a filesystem concept — they give branches physical locations on disk.
- You always need branches. You need worktrees when you need **multiple branches checked out simultaneously**.

### CI/CD Considerations

- CI runners typically clone fresh per build, making worktrees less relevant in CI.
- For **local CI simulation** (running tests on multiple branches before pushing), worktrees shine.
- Some teams use worktrees in CI to optimize multi-branch build pipelines by sharing the `.git` database.
- For Harbangan's Docker-based workflow, each worktree would need its own `cargo build` artifacts and `node_modules`, but would share the same Docker daemon.

---

## 5. Conflict Resolution Patterns

### Prevention: The Best Strategy

The most effective approach is to **prevent conflicts** rather than resolve them:

1. **Strict file ownership:** Assign each file to exactly one agent. The Harbangan project's `.claude/skills/team-coordination/` already includes file ownership mapping — this is the right pattern.
2. **Wave-based execution:** Structure work in waves where dependent tasks run sequentially, independent tasks run in parallel (the plan-mode wave structure in Harbangan's CLAUDE.md).
3. **Small, focused PRs:** Each agent produces one small PR touching a limited set of files.

### Early Detection: Clash

[Clash](https://github.com/clash-sh/clash) is a Rust-based tool purpose-built for this problem:
- Uses `git merge-tree` (via the `gix` library) to perform three-way merges between all worktree pairs **without modifying the repository** (100% read-only).
- Can be integrated as a pre-write hook — automatically checks for conflicts before every AI agent edit.
- Provides a **conflict matrix** showing which branches conflict with each other.
- Single binary, no runtime dependencies.

### Resolution Strategies

When conflicts do occur across worktrees:

1. **Rebase early and often:** Periodically rebase feature branches onto the latest main to keep divergence small.
2. **Isolated merge branches:** Create a temporary branch from the feature branch to resolve conflicts, keeping the feature branch clean.
3. **Sequential merge order:** Merge the most foundational changes first (e.g., backend types before frontend consumers), then rebase remaining branches.
4. **Coordinator role:** Have a human or lead agent review all PRs and determine merge order based on dependency analysis.
5. **Semantic conflict awareness:** Even without textual conflicts, parallel changes can cause semantic issues (e.g., two agents adding the same function name). Automated testing after merge catches these.

### Post-Merge Verification

After merging parallel worktree changes:
- Run the full test suite (`cargo test --lib`, `npm run build && npm run lint`).
- Run integration/E2E tests if available.
- Check for semantic conflicts that Git merge cannot detect.

---

## Recommendations for Harbangan

Based on this research, here are specific recommendations for the Harbangan project:

### 1. Leverage Existing Infrastructure

Harbangan's multi-agent system already has the right building blocks:
- **Agent definitions** in `.claude/agents/` — add `isolation: worktree` to frontmatter for agents that should run in parallel.
- **File ownership** in `.claude/skills/team-coordination/` — already maps files to agents.
- **Wave-based task decomposition** in `.claude/rules/plan-mode.md` — already structures work to minimize conflicts.

### 2. Recommended Worktree Strategy

```
harbangan/                          ← Main working directory (coordinator/human)
harbangan/.trees/
├── backend-feat/                   ← rust-backend-engineer worktree
├── frontend-feat/                  ← react-frontend-engineer worktree
├── backend-tests/                  ← backend-qa worktree
└── infra/                          ← devops-engineer worktree
```

### 3. Agent Definition Updates

Add `isolation: worktree` to parallel-capable agents:
- `rust-backend-engineer.md`
- `react-frontend-engineer.md`
- `backend-qa.md` (after backend work merges)
- `frontend-qa.md` (after frontend work merges)

Keep the `scrum-master.md` agent in the main worktree as coordinator.

### 4. Environment Concerns

- **Backend:** Each worktree needs `cargo build` — the `target/` directory is per-worktree. First build will be slow; subsequent builds benefit from shared compiled dependencies.
- **Frontend:** Each worktree needs `npm install`. Consider a post-checkout hook.
- **Database:** All worktrees share the same PostgreSQL instance. For dev/test, this is fine if agents work on different tables/features. For schema migrations, serialize these through the coordinator.

### 5. Conflict Mitigation

- Install Clash for early conflict detection across worktrees.
- Maintain strict file ownership per agent.
- Merge backend foundational changes before frontend consumer changes (wave ordering).
- Run full quality gates (`cargo clippy`, `cargo test --lib`, `npm run build`, `npm run lint`) on each PR before merge.

---

## Sources

- [Using Git Worktrees for Multi-Feature Development with AI Agents - Nick Mitchinson](https://www.nrmitchi.com/2025/10/using-git-worktrees-for-multi-feature-development-with-ai-agents/)
- [Git Worktree Tutorial - DataCamp](https://www.datacamp.com/tutorial/git-worktree-tutorial)
- [Using Git Worktrees for Concurrent Development - Ken Muse](https://www.kenmuse.com/blog/using-git-worktrees-for-concurrent-development/)
- [Mastering Git Worktrees with Claude Code - Dogukan Uraz Tuna](https://medium.com/@dtunai/mastering-git-worktrees-with-claude-code-for-parallel-development-workflow-41dc91e645fe)
- [How Git Worktrees Changed My AI Agent Workflow - Nx Blog](https://nx.dev/blog/git-worktrees-ai-agents)
- [Create custom subagents - Claude Code Docs](https://code.claude.com/docs/en/sub-agents)
- [Orchestrate teams of Claude Code sessions - Claude Code Docs](https://code.claude.com/docs/en/agent-teams)
- [Common workflows - Claude Code Docs](https://code.claude.com/docs/en/common-workflows)
- [Claude Code Worktrees: Run Parallel Sessions Without Conflicts](https://claudefa.st/blog/guide/development/worktree-guide)
- [Worktrees: Parallel Agent Isolation - Agent Factory](https://agentfactory.panaversity.org/docs/General-Agents-Foundations/general-agents/worktrees)
- [Agent System & Subagents - DeepWiki](https://deepwiki.com/anthropics/claude-code/3.1-ai-models-and-execution-strategies)
- [How we're shipping faster with Claude Code and Git Worktrees - incident.io](https://incident.io/blog/shipping-faster-with-claude-code-and-git-worktrees)
- [Parallel AI Coding with Git Worktrees and Custom Claude Code Commands - Agent Interviews](https://docs.agentinterviews.com/blog/parallel-ai-coding-with-gitworktrees/)
- [Git worktrees for parallel AI coding agents - Upsun Developer Center](https://devcenter.upsun.com/posts/git-worktrees-for-parallel-ai-coding-agents/)
- [GitHub - clash-sh/clash: Avoid merge conflicts across git worktrees](https://github.com/clash-sh/clash)
- [Agent Teams or: How I Learned to Stop Worrying About Merge Conflicts - Intility Engineering](https://engineering.intility.com/article/agent-teams-or-how-i-learned-to-stop-worrying-about-merge-conflicts-and-love-git-worktrees)
- [GitHub - nwiizo/ccswarm: Multi-agent orchestration with Git worktree isolation](https://github.com/nwiizo/ccswarm)
- [Feature: Enter/resume existing worktrees - GitHub Issue #31969](https://github.com/anthropics/claude-code/issues/31969)
- [Parallel Development with ClaudeCode and Git Worktrees - Yee Fei](https://medium.com/@ooi_yee_fei/parallel-ai-development-with-git-worktrees-f2524afc3e33)
- [Git Worktree Parallel Development - Developer Toolkit](https://developertoolkit.ai/en/codex/advanced-techniques/worktrees/)
- [Git - git-worktree Documentation](https://git-scm.com/docs/git-worktree)
- [Claude Code Git Worktree Support - SuperGok](https://supergok.com/claude-code-git-worktree-support/)
- [Parallel Workflows: Git Worktrees and the Art of Managing Multiple AI Agents - Dennis Somerville](https://medium.com/@dennis.somerville/parallel-workflows-git-worktrees-and-the-art-of-managing-multiple-ai-agents-6fa3dc5eec1d)
