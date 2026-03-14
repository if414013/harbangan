---
name: rename-plan
description: Rename plan files from random generated names to datetime-prefixed meaningful names (e.g., humble-munching-bachman.md → 2026-03-14-my-oauth-refactor.md)
argument-hint: "<description>"
allowed-tools:
  - Bash
  - Glob
  - AskUserQuestion
---

# Rename Plan File

Rename the most recently modified `.md` file in `.claude/plans/` to a datetime-prefixed descriptive name.

## Steps

1. **Get description** — use `$ARGUMENTS` as the description. If empty, use `AskUserQuestion` to ask:
   - Question: "What should this plan be named?"
   - Header: "Plan name"
   - Options: suggest 2-3 names based on the plan file's content (read the first few lines), plus an "Other" option for custom input.

2. **Find the most recent plan file** — use `Glob` to list `**/*.md` in `.claude/plans/`, then use `Bash` with `ls -t` to find the most recently modified one.

3. **Generate the new filename**:
   - Date prefix: use `date +%Y-%m-%d` via Bash
   - Sanitize the description: lowercase, replace spaces with hyphens, strip characters that aren't `[a-z0-9-]`, collapse multiple hyphens, trim leading/trailing hyphens
   - Final name: `YYYY-MM-DD-<sanitized-description>.md`

4. **Handle duplicates** — if the target filename already exists, append `-2`, `-3`, etc. until a unique name is found.

5. **Rename** — use `Bash` with `mv` to rename the file within `.claude/plans/`.

6. **Report** — tell the user the old and new filenames.

## Example

```
/rename-plan my oauth refactor

# Finds: .claude/plans/humble-munching-bachman.md (most recent)
# Renames to: .claude/plans/2026-03-14-my-oauth-refactor.md
```
