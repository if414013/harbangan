#!/usr/bin/env bash
#
# verify.sh — Run all quality checks for the rkgw project.
#
# Usage:
#   ./verify.sh              # Check all services (backend + frontend)
#   ./verify.sh backend      # Check backend only
#   ./verify.sh frontend     # Check frontend only
#   ./verify.sh all          # Check all services (explicit)
#
# Exit codes:
#   0 — All checks passed
#   1 — One or more checks failed
#   2 — Invalid usage or missing project directory

set -euo pipefail

# ---------------------------------------------------------------------------
# Color support
# ---------------------------------------------------------------------------
if [[ -t 1 ]] && command -v tput &>/dev/null && [[ $(tput colors 2>/dev/null || echo 0) -ge 8 ]]; then
    GREEN=$(tput setaf 2)
    RED=$(tput setaf 1)
    BOLD=$(tput bold)
    RESET=$(tput sgr0)
else
    GREEN=""
    RED=""
    BOLD=""
    RESET=""
fi

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
FAILED=0
SERVICE="${1:-all}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
usage() {
    echo "Usage: $0 [backend|frontend|all]"
    exit 2
}

run_check() {
    local label="$1"
    shift
    local cmd="$*"

    printf "%s=> %s%s\n" "$BOLD" "$label" "$RESET"
    printf "   %s\n" "$cmd"

    if eval "$cmd"; then
        printf "   %sPASS%s  %s\n\n" "$GREEN" "$RESET" "$label"
    else
        printf "   %sFAIL%s  %s\n\n" "$RED" "$RESET" "$label"
        FAILED=1
    fi
}

# ---------------------------------------------------------------------------
# Validate environment
# ---------------------------------------------------------------------------
# Find project root (directory containing both backend/ and frontend/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"

if [[ ! -d "$PROJECT_ROOT/backend" ]] || [[ ! -d "$PROJECT_ROOT/frontend" ]]; then
    echo "${RED}Error:${RESET} Cannot find backend/ and frontend/ directories."
    echo "Expected project root: $PROJECT_ROOT"
    exit 2
fi

case "$SERVICE" in
    backend|frontend|all) ;;
    *) usage ;;
esac

echo "${BOLD}================================================================${RESET}"
echo "${BOLD} rkgw verification — service: ${SERVICE}${RESET}"
echo "${BOLD}================================================================${RESET}"
echo ""

# ---------------------------------------------------------------------------
# Backend checks
# ---------------------------------------------------------------------------
if [[ "$SERVICE" == "backend" ]] || [[ "$SERVICE" == "all" ]]; then
    echo "${BOLD}--- Backend ---${RESET}"
    echo ""
    run_check "cargo clippy (warnings as errors)" \
        "cd '$PROJECT_ROOT/backend' && cargo clippy -- -D warnings"
    run_check "cargo fmt (check formatting)" \
        "cd '$PROJECT_ROOT/backend' && cargo fmt --check"
    run_check "cargo test --lib (unit tests)" \
        "cd '$PROJECT_ROOT/backend' && cargo test --lib"
fi

# ---------------------------------------------------------------------------
# Frontend checks
# ---------------------------------------------------------------------------
if [[ "$SERVICE" == "frontend" ]] || [[ "$SERVICE" == "all" ]]; then
    echo "${BOLD}--- Frontend ---${RESET}"
    echo ""
    run_check "npm run lint (eslint)" \
        "cd '$PROJECT_ROOT/frontend' && npm run lint"
    run_check "npm run build (tsc + vite)" \
        "cd '$PROJECT_ROOT/frontend' && npm run build"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo "${BOLD}================================================================${RESET}"
if [[ "$FAILED" -eq 0 ]]; then
    echo "${GREEN}${BOLD}All checks passed.${RESET}"
else
    echo "${RED}${BOLD}One or more checks failed.${RESET}"
fi
echo "${BOLD}================================================================${RESET}"

exit "$FAILED"
