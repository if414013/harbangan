# Worktree-Aware Multi-Agent Workflow

Per-team worktrees for parallel multi-feature development. First team runs normally; subsequent teams auto-detect and get worktrees. Small fix/refactor = no worktree.

## Files to Modify

### `.claude/skills/team-spawn/SKILL.md`
- Add `--worktree` / `--no-worktree` flags to argument-hint
- **New Step 3.5: Worktree Resolution** (between name gen and spawn):
  - Auto-detect: list `~/.claude/teams/`, check if any team has agents not in `replaced/exited/shutdown` status
  - Skip auto-detect for presets `review`, `debug`, `security`, `research` (read-only/ephemeral)
  - `--worktree` forces it, `--no-worktree` skips, otherwise auto-detect decides
  - Accept `--feature-name` from team-feature for branch naming; fallback to team-name
  - Create: `git worktree add .trees/{team-name} -b feat/{feature-short-name}`
  - If branch exists, retry with `-{short-id}` suffix
  - If stale worktree at path, prune first: `git worktree prune && git worktree remove .trees/{team-name} --force`
  - Auto-setup: `cd .trees/{team-name}/backend && cargo build` + `cd .trees/{team-name}/frontend && npm install` (run sequentially, report per-step success)
  - On setup failure: warn, ask user to proceed or abort
- **Modify Step 4**: change `cd {project-root}` → `cd {working-dir}` where `working-dir = worktree.path ?? project-root`
- **Modify Step 4.5** (respawn): gather git log from `{working-dir}`, not project-root
- **Modify Step 6** (config schema): add `"worktree": { "path", "branch", "created_at" } | null`
- **Modify Step 7** (report): include worktree path, branch, build status

### `.claude/skills/team-feature/SKILL.md`
- **Modify Step 5**: pass `--feature-name "{sanitized-desc}"` to `/team-spawn`; forward `--worktree` if provided
- **Modify Step 7**: verification runs in `{working-dir}/{service-subdir}` (read worktree.path from config)
- **Add Step 7.5**: if worktree active, push branch + `gh pr create` from worktree dir

### `.claude/skills/team-shutdown/SKILL.md`
- **Modify Step 2**: show worktree status (uncommitted changes, unpushed commits, PR state)
- **Add Step 4.5: Worktree Cleanup** (after GitHub persist, before config removal):
  - Check uncommitted changes → offer to commit
  - Check unpushed commits → offer to push
  - Check PR status via `gh pr list --head {branch}`
  - `git worktree remove .trees/{team-name} --force`
  - Delete local branch only if PR merged; otherwise preserve
  - `git worktree prune`
- **Modify Step 5** (report): include worktree cleanup status

### `.claude/skills/team-status/SKILL.md`
- **Modify Step 3.5**: agent activity probe uses `{working-dir}` paths for file mtime checks and git log
- **Modify Step 5**: add `Worktree:` line showing path, branch, clean/dirty status

### `.claude/skills/team-coordination/references/merge-strategies.md`
- **Add Pattern 4: Worktree Isolation** after Pattern 3 — per-team worktrees, merge flow (sequential PR merges with rebase), lifecycle diagram

### `.claude/skills/team-coordination/SKILL.md` line 195
- Update reference line: "3 integration patterns" → "4 integration patterns (direct, sub-branch, trunk-based, worktree isolation)"

### `.claude/agents/scrum-master.md`
- **Add "Worktree Awareness" section** after "Cross-Service Awareness": first team = main dir, subsequent = `.trees/`, verification in worktree dir, cross-team file ownership must not overlap, serialize migrations

### `.gitignore` (near line 42)
- Add `.trees/` after existing `.worktrees/`

## Verification
```bash
grep "worktree" .claude/skills/team-spawn/SKILL.md | head -3    # expect matches
grep "working-dir" .claude/skills/team-spawn/SKILL.md | head -1 # expect match
grep "Pattern 4" .claude/skills/team-coordination/references/merge-strategies.md # expect match
grep ".trees" .gitignore                                          # expect match
grep "Worktree Awareness" .claude/agents/scrum-master.md          # expect match
```
