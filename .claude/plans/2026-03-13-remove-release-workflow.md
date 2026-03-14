# Remove Leftover Release Workflow

## Context

Harbangan has moved to Docker-based deployment (`docker-compose.yml` / `docker-compose.gateway.yml`). The entire `.github/workflows/release.yml` is a leftover from when it was distributed as a standalone binary via GitHub Releases and Homebrew. None of it is needed anymore.

## Change

**Delete:** `.github/workflows/release.yml` (entire file)

This removes:
- Cross-platform binary builds (macOS Intel/ARM, Linux Intel/ARM)
- GitHub Release creation with `.tar.gz` artifacts
- Homebrew formula generation and tap publishing

## Post-cleanup (manual, outside repo)

- Remove `HOMEBREW_TAP_TOKEN` secret from GitHub repo settings
- Consider archiving the `homebrew-tvps` tap repo

## Verification

- Confirm `.github/workflows/ci.yml` still exists (CI is separate and unaffected)
- No other files reference `release.yml`
