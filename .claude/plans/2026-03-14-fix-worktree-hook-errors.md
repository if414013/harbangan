# Fix worktree hook errors + add-to-project CI failure

## 1. Worktree guard Stop hook

The claude-mem plugin fires a Stop hook that fails in worktrees because the transcript path is derived from the worktree CWD. Add a worktree-aware guard.

- **`.claude/settings.json`** (line 68, before closing `}` of hooks)
  - Add a `Stop` hook array with a command hook pointing to a new guard script
  - The Stop hook runs before the claude-mem plugin's internal hook

- **`.claude/hooks/worktree-guard-stop.sh`** (new file)
  - Read `cwd` from stdin JSON
  - If CWD contains `/.claude/worktrees/`, exit with code 2 (block) to prevent downstream Stop hooks from running
  - Otherwise exit 0 (allow)

## 2. Fix add-to-project CI workflow

`GITHUB_TOKEN` can't access user-level projects. Need a fine-grained PAT.

- **`.github/workflows/project-automation.yml`** (lines 13-16)
  - Update action from `actions/add-to-project@v1` to `actions/add-to-project@v1.0.2`
  - Add `permissions: projects: write` at job level
  - Keep using `PROJECT_TOKEN` secret (user must create fine-grained PAT with `project` read/write scope and store as repo secret)

- **Manual step (not automatable):**
  - User creates fine-grained PAT at github.com/settings/tokens with:
    - Repository access: `if414013/harbangan`
    - Permissions: Projects → Read and write
  - Store as `PROJECT_TOKEN` repo secret

## 3. Fix block-sensitive-commits.sh false positive

- **`.claude/hooks/block-sensitive-commits.sh`**
  - Fix the regex that blocks `git add .` to not match `git add .claude/...`
  - Change pattern from `git add \.` to `git add \.$` (only match literal `git add .` at end)

## Verification
```bash
# Hook test: simulate worktree CWD
echo '{"cwd":"/path/.claude/worktrees/test"}' | .claude/hooks/worktree-guard-stop.sh; echo $?
# Should output exit code 2

# CI: validate workflow syntax
gh workflow view project-automation.yml 2>&1 | head -5
```
