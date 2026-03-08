#!/usr/bin/env bash
#
# run-checks.sh — Run all lint/test/build checks and output a summary.
#
# Runs every check regardless of individual failures, then prints a summary
# table with pass/fail counts.
#
# Usage:
#   ./run-checks.sh
#
# Exit codes:
#   0 — All checks passed
#   1 — One or more checks failed
#   2 — Missing project directory

# Do NOT use "set -e" — we intentionally continue past failures.
set -uo pipefail

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
declare -a CHECK_NAMES=()
declare -a CHECK_RESULTS=()
PASS_COUNT=0
FAIL_COUNT=0

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
run_check() {
    local label="$1"
    shift
    local cmd="$*"

    printf "%s=> %s%s\n" "$BOLD" "$label" "$RESET"
    printf "   %s\n" "$cmd"

    if eval "$cmd"; then
        CHECK_NAMES+=("$label")
        CHECK_RESULTS+=("PASS")
        PASS_COUNT=$((PASS_COUNT + 1))
        printf "   %sPASS%s  %s\n\n" "$GREEN" "$RESET" "$label"
    else
        CHECK_NAMES+=("$label")
        CHECK_RESULTS+=("FAIL")
        FAIL_COUNT=$((FAIL_COUNT + 1))
        printf "   %sFAIL%s  %s\n\n" "$RED" "$RESET" "$label"
    fi
}

# ---------------------------------------------------------------------------
# Validate environment
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"

if [[ ! -d "$PROJECT_ROOT/backend" ]] || [[ ! -d "$PROJECT_ROOT/frontend" ]]; then
    echo "${RED}Error:${RESET} Cannot find backend/ and frontend/ directories."
    echo "Expected project root: $PROJECT_ROOT"
    exit 2
fi

echo "${BOLD}================================================================${RESET}"
echo "${BOLD} rkgw quality checks${RESET}"
echo "${BOLD}================================================================${RESET}"
echo ""

# ---------------------------------------------------------------------------
# Backend checks
# ---------------------------------------------------------------------------
echo "${BOLD}--- Backend ---${RESET}"
echo ""

run_check "cargo clippy (warnings as errors)" \
    "cd '$PROJECT_ROOT/backend' && cargo clippy -- -D warnings"

run_check "cargo fmt (check formatting)" \
    "cd '$PROJECT_ROOT/backend' && cargo fmt --check"

run_check "cargo test --lib (unit tests)" \
    "cd '$PROJECT_ROOT/backend' && cargo test --lib"

# ---------------------------------------------------------------------------
# Frontend checks
# ---------------------------------------------------------------------------
echo "${BOLD}--- Frontend ---${RESET}"
echo ""

run_check "npm run lint (eslint)" \
    "cd '$PROJECT_ROOT/frontend' && npm run lint"

run_check "npm run build (tsc + vite)" \
    "cd '$PROJECT_ROOT/frontend' && npm run build"

# ---------------------------------------------------------------------------
# Summary table
# ---------------------------------------------------------------------------
TOTAL=$((PASS_COUNT + FAIL_COUNT))

echo "${BOLD}================================================================${RESET}"
echo "${BOLD} Summary${RESET}"
echo "${BOLD}================================================================${RESET}"
printf "%-45s %s\n" "Check" "Result"
printf "%-45s %s\n" "---------------------------------------------" "------"

for i in "${!CHECK_NAMES[@]}"; do
    result="${CHECK_RESULTS[$i]}"
    if [[ "$result" == "PASS" ]]; then
        color="$GREEN"
    else
        color="$RED"
    fi
    printf "%-45s %s%s%s\n" "${CHECK_NAMES[$i]}" "$color" "$result" "$RESET"
done

echo ""
printf "Total: %d   ${GREEN}Passed: %d${RESET}   ${RED}Failed: %d${RESET}\n" \
    "$TOTAL" "$PASS_COUNT" "$FAIL_COUNT"
echo "${BOLD}================================================================${RESET}"

if [[ "$FAIL_COUNT" -gt 0 ]]; then
    echo "${RED}${BOLD}Some checks failed.${RESET}"
    exit 1
else
    echo "${GREEN}${BOLD}All checks passed.${RESET}"
    exit 0
fi
