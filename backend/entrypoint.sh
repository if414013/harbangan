#!/bin/sh
set -eu

# Proxy-only entrypoint for harbangan backend.
# Runs device code flows for unconfigured providers (Kiro, Copilot),
# caches credentials to /data/tokens.json for reuse across restarts,
# then launches the gateway binary with the obtained credentials.

TOKEN_CACHE="/data/tokens.json"
OIDC_REGION="${KIRO_SSO_REGION:-${KIRO_REGION:-us-east-1}}"
OIDC_BASE="https://oidc.${OIDC_REGION}.amazonaws.com"

# GitHub Copilot device flow constants
# Public OAuth app client ID for GitHub's device flow (not a secret)
COPILOT_GITHUB_CLIENT_ID="${COPILOT_GITHUB_CLIENT_ID:-Iv1.b507a08c87ecfe98}"
COPILOT_DEFAULT_BASE_URL="https://api.githubcopilot.com"

# ── Validate ─────────────────────────────────────────────────────────
if [ -z "${PROXY_API_KEY:-}" ]; then
    echo "ERROR: PROXY_API_KEY is required" >&2
    exit 1
fi

# ── Token cache helpers ──────────────────────────────────────────────
# The cache is a single JSON object with per-provider keys:
#   { "kiro": {...}, "copilot": {...} }

ensure_cache_dir() {
    mkdir -p "$(dirname "$TOKEN_CACHE")"
}

read_cache_field() {
    # read_cache_field <provider> <field>
    if [ -f "$TOKEN_CACHE" ]; then
        jq -r ".[\"$1\"][\"$2\"] // empty" "$TOKEN_CACHE" 2>/dev/null
    fi
}

write_cache_provider() {
    # write_cache_provider <provider> <json_object>
    ensure_cache_dir
    if [ -f "$TOKEN_CACHE" ]; then
        EXISTING=$(cat "$TOKEN_CACHE")
    else
        EXISTING="{}"
    fi
    (umask 077 && echo "$EXISTING" | jq --argjson val "$2" ".[\"$1\"] = \$val" > "${TOKEN_CACHE}.tmp" \
        && mv "${TOKEN_CACHE}.tmp" "$TOKEN_CACHE")
}

# ── Kiro: load + validate cached tokens ──────────────────────────────
load_cached_kiro_tokens() {
    CACHED_REFRESH=$(read_cache_field kiro refresh_token)
    CACHED_OIDC_ID=$(read_cache_field kiro client_id)
    CACHED_OIDC_SEC=$(read_cache_field kiro client_secret)
    if [ -n "$CACHED_REFRESH" ] && [ -n "$CACHED_OIDC_ID" ] && [ -n "$CACHED_OIDC_SEC" ]; then
        return 0
    fi
    # Migrate legacy flat format (pre-multi-provider cache)
    if [ -f "$TOKEN_CACHE" ]; then
        CACHED_REFRESH=$(jq -r '.refresh_token // empty' "$TOKEN_CACHE" 2>/dev/null)
        CACHED_OIDC_ID=$(jq -r '.client_id // empty' "$TOKEN_CACHE" 2>/dev/null)
        CACHED_OIDC_SEC=$(jq -r '.client_secret // empty' "$TOKEN_CACHE" 2>/dev/null)
        if [ -n "$CACHED_REFRESH" ] && [ -n "$CACHED_OIDC_ID" ] && [ -n "$CACHED_OIDC_SEC" ]; then
            return 0
        fi
    fi
    return 1
}

validate_cached_kiro_tokens() {
    echo "→ Validating cached Kiro credentials..."
    VALIDATE_RESPONSE=$(curl -s -X POST "${OIDC_BASE}/token" \
        -H "Content-Type: application/json" \
        -d "{\"grantType\":\"refresh_token\",\"clientId\":\"${CACHED_OIDC_ID}\",\"clientSecret\":\"${CACHED_OIDC_SEC}\",\"refreshToken\":\"${CACHED_REFRESH}\"}")

    VALIDATED_ACCESS=$(echo "$VALIDATE_RESPONSE" | jq -r '.accessToken // empty')
    if [ -n "$VALIDATED_ACCESS" ]; then
        NEW_REFRESH=$(echo "$VALIDATE_RESPONSE" | jq -r '.refreshToken // empty')
        if [ -n "$NEW_REFRESH" ]; then
            CACHED_REFRESH="$NEW_REFRESH"
            save_kiro_tokens "$CACHED_REFRESH" "$CACHED_OIDC_ID" "$CACHED_OIDC_SEC"
        fi
        echo "  Cached Kiro credentials valid"
        return 0
    fi
    echo "  Cached Kiro credentials expired or invalid"
    return 1
}

save_kiro_tokens() {
    write_cache_provider kiro "$(jq -n --arg rt "$1" --arg ci "$2" --arg cs "$3" \
        '{refresh_token: $rt, client_id: $ci, client_secret: $cs}')"
}

# ══════════════════════════════════════════════════════════════════════
# Provider 1: Kiro (AWS SSO OIDC device code flow)
# ══════════════════════════════════════════════════════════════════════
if [ "${KIRO_ENABLED:-true}" = "false" ]; then
    echo "→ Kiro provider disabled"
elif [ -z "${KIRO_REFRESH_TOKEN:-}" ]; then
    if load_cached_kiro_tokens && validate_cached_kiro_tokens; then
        export KIRO_REFRESH_TOKEN="$CACHED_REFRESH"
        export KIRO_CLIENT_ID="$CACHED_OIDC_ID"
        export KIRO_CLIENT_SECRET="$CACHED_OIDC_SEC"
    else
        echo ""
        echo "┌─────────────────────────────────────────────────────────┐"
        echo "│  Kiro Gateway — Proxy-Only Mode                         │"
        echo "├─────────────────────────────────────────────────────────┤"
        echo "│  KIRO_REGION:    ${KIRO_REGION:-us-east-1}"
        echo "│  OIDC_REGION:    ${OIDC_REGION}"
        if [ -n "${KIRO_SSO_URL:-}" ]; then
            echo "│  KIRO_SSO_URL:   ${KIRO_SSO_URL:-}"
            echo "│  Login mode:     Identity Center (pro)"
        else
            echo "│  Login mode:     Builder ID (free)"
        fi
        echo "└─────────────────────────────────────────────────────────┘"
        echo ""

        # Step 1: Register OIDC client
        echo "→ Registering OIDC client at ${OIDC_BASE}..."

        REGISTER_BODY="{\"clientName\":\"harbangan-proxy\",\"clientType\":\"public\",\"scopes\":[\"codewhisperer:completions\",\"codewhisperer:analysis\",\"codewhisperer:conversations\"],\"grantTypes\":[\"urn:ietf:params:oauth:grant-type:device_code\",\"refresh_token\"]"

        if [ -n "${KIRO_SSO_URL:-}" ]; then
            REGISTER_BODY="${REGISTER_BODY},\"issuerUrl\":\"${KIRO_SSO_URL:-}\""
        fi
        REGISTER_BODY="${REGISTER_BODY}}"

        REG_RESPONSE=$(curl -sf -X POST "${OIDC_BASE}/client/register" \
            -H "Content-Type: application/json" \
            -d "$REGISTER_BODY") || {
            echo "ERROR: OIDC client registration failed" >&2
            exit 1
        }

        OIDC_CID=$(echo "$REG_RESPONSE" | jq -r '.clientId')
        OIDC_CSEC=$(echo "$REG_RESPONSE" | jq -r '.clientSecret')

        if [ -z "$OIDC_CID" ] || [ "$OIDC_CID" = "null" ]; then
            echo "ERROR: Failed to parse client registration response" >&2
            echo "$REG_RESPONSE" >&2
            exit 1
        fi

        echo "  Client registered (${OIDC_CID%${OIDC_CID#????????}}...)"

        # Step 2: Start device authorization
        START_URL="${KIRO_SSO_URL:-https://view.awsapps.com/start}"

        DEVICE_RESPONSE=$(curl -sf -X POST "${OIDC_BASE}/device_authorization" \
            -H "Content-Type: application/json" \
            -d "{\"clientId\":\"${OIDC_CID}\",\"clientSecret\":\"${OIDC_CSEC}\",\"startUrl\":\"${START_URL}\"}") || {
            echo "ERROR: Device authorization failed" >&2
            exit 1
        }

        DEVICE_CODE=$(echo "$DEVICE_RESPONSE" | jq -r '.deviceCode')
        USER_CODE=$(echo "$DEVICE_RESPONSE" | jq -r '.userCode')
        VERIFY_URL=$(echo "$DEVICE_RESPONSE" | jq -r '.verificationUriComplete')
        EXPIRES_IN=$(echo "$DEVICE_RESPONSE" | jq -r '.expiresIn')
        INTERVAL=$(echo "$DEVICE_RESPONSE" | jq -r '.interval')

        echo ""
        echo "╔═══════════════════════════════════════════════════════════╗"
        echo "║  [Kiro] Open this URL in your browser to authorize:      ║"
        echo "║                                                          ║"
        echo "║  $VERIFY_URL"
        echo "║                                                          ║"
        echo "║  User code: $USER_CODE"
        echo "╚═══════════════════════════════════════════════════════════╝"
        echo ""
        echo "→ Waiting for Kiro authorization (expires in ${EXPIRES_IN}s)..."

        # Step 3: Poll for token
        ELAPSED=0
        while [ "$ELAPSED" -lt "$EXPIRES_IN" ]; do
            sleep "$INTERVAL"
            ELAPSED=$((ELAPSED + INTERVAL))

            TOKEN_RESPONSE=$(curl -s -X POST "${OIDC_BASE}/token" \
                -H "Content-Type: application/json" \
                -d "{\"grantType\":\"urn:ietf:params:oauth:grant-type:device_code\",\"clientId\":\"${OIDC_CID}\",\"clientSecret\":\"${OIDC_CSEC}\",\"deviceCode\":\"${DEVICE_CODE}\"}")

            ACCESS_TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.accessToken // empty')
            if [ -n "$ACCESS_TOKEN" ]; then
                REFRESH_TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.refreshToken // empty')
                echo ""
                echo "  Kiro authorization successful"
                echo ""
                break
            fi

            if echo "$TOKEN_RESPONSE" | grep -q "slow_down"; then
                INTERVAL=$((INTERVAL + 1))
                continue
            fi

            if echo "$TOKEN_RESPONSE" | grep -q "authorization_pending"; then
                continue
            fi

            echo "ERROR: Kiro token polling failed:" >&2
            echo "$TOKEN_RESPONSE" >&2
            exit 1
        done

        if [ -z "${REFRESH_TOKEN:-}" ]; then
            echo "ERROR: Kiro device authorization timed out. Please restart and try again." >&2
            exit 1
        fi

        save_kiro_tokens "$REFRESH_TOKEN" "$OIDC_CID" "$OIDC_CSEC"
        echo "  Kiro credentials cached to ${TOKEN_CACHE}"

        export KIRO_REFRESH_TOKEN="$REFRESH_TOKEN"
        export KIRO_CLIENT_ID="$OIDC_CID"
        export KIRO_CLIENT_SECRET="$OIDC_CSEC"
    fi
fi

# ══════════════════════════════════════════════════════════════════════
# Provider 2: GitHub Copilot (GitHub device code flow)
# ══════════════════════════════════════════════════════════════════════
if [ "${COPILOT_ENABLED:-false}" = "false" ]; then
    echo "→ Copilot provider disabled"
elif [ -n "${COPILOT_TOKEN:-}" ]; then
    export COPILOT_BASE_URL="${COPILOT_BASE_URL:-${COPILOT_DEFAULT_BASE_URL}}"
    echo "→ Copilot token provided via env"
elif CACHED_CP_TK=$(read_cache_field copilot token) && [ -n "$CACHED_CP_TK" ]; then
    CACHED_CP_BASE=$(read_cache_field copilot base_url)
    echo "→ Loaded cached Copilot token"
    export COPILOT_TOKEN="$CACHED_CP_TK"
    export COPILOT_BASE_URL="${CACHED_CP_BASE:-${COPILOT_DEFAULT_BASE_URL}}"
fi

# ── Interactive device flows ─────────────────────────────────────────
# Copilot device flow requires user interaction. It only runs
# when the token is not set and not cached.

run_copilot_device_flow() {
    echo ""
    echo "┌─────────────────────────────────────────────────────────┐"
    echo "│  GitHub Copilot — Device Code Authorization              │"
    echo "└─────────────────────────────────────────────────────────┘"
    echo ""

    # Step 1: Request device code from GitHub
    echo "→ Requesting GitHub device code..."
    DEVICE_RESPONSE=$(curl -sf -X POST "https://github.com/login/device/code" \
        -H "Accept: application/json" \
        -H "Content-Type: application/json" \
        -d "{\"client_id\":\"${COPILOT_GITHUB_CLIENT_ID}\",\"scope\":\"read:user\"}") || {
        echo "ERROR: GitHub device code request failed" >&2
        return 1
    }

    GH_DEVICE_CODE=$(echo "$DEVICE_RESPONSE" | jq -r '.device_code // empty')
    GH_USER_CODE=$(echo "$DEVICE_RESPONSE" | jq -r '.user_code // empty')
    GH_VERIFY_URL=$(echo "$DEVICE_RESPONSE" | jq -r '.verification_uri // empty')
    GH_EXPIRES_IN=$(echo "$DEVICE_RESPONSE" | jq -r '.expires_in // 900')
    GH_INTERVAL=$(echo "$DEVICE_RESPONSE" | jq -r '.interval // 5')

    if [ -z "$GH_DEVICE_CODE" ] || [ -z "$GH_USER_CODE" ]; then
        echo "ERROR: Failed to parse GitHub device code response" >&2
        echo "$DEVICE_RESPONSE" >&2
        return 1
    fi

    echo ""
    echo "╔═══════════════════════════════════════════════════════════╗"
    echo "║  [Copilot] Open this URL and enter the code:             ║"
    echo "║                                                          ║"
    echo "║  URL:  ${GH_VERIFY_URL}"
    echo "║  Code: ${GH_USER_CODE}"
    echo "╚═══════════════════════════════════════════════════════════╝"
    echo ""
    echo "→ Waiting for GitHub authorization (expires in ${GH_EXPIRES_IN}s)..."

    # Step 2: Poll for GitHub access token
    GH_ELAPSED=0
    GH_ACCESS=""
    while [ "$GH_ELAPSED" -lt "$GH_EXPIRES_IN" ]; do
        sleep "$GH_INTERVAL"
        GH_ELAPSED=$((GH_ELAPSED + GH_INTERVAL))

        GH_RESP=$(curl -s -X POST "https://github.com/login/oauth/access_token" \
            -H "Accept: application/json" \
            -H "Content-Type: application/json" \
            -d "{\"client_id\":\"${COPILOT_GITHUB_CLIENT_ID}\",\"device_code\":\"${GH_DEVICE_CODE}\",\"grant_type\":\"urn:ietf:params:oauth:grant-type:device_code\"}")

        GH_ERROR=$(echo "$GH_RESP" | jq -r '.error // empty')

        if [ -z "$GH_ERROR" ] || [ "$GH_ERROR" = "null" ]; then
            GH_ACCESS=$(echo "$GH_RESP" | jq -r '.access_token // empty')
            if [ -n "$GH_ACCESS" ]; then
                echo "  GitHub authorization successful"
                break
            fi
        fi

        case "$GH_ERROR" in
            authorization_pending) continue ;;
            slow_down) GH_INTERVAL=$((GH_INTERVAL + 5)); continue ;;
            *)
                echo "ERROR: GitHub token polling failed: ${GH_ERROR}" >&2
                return 1
                ;;
        esac
    done

    if [ -z "$GH_ACCESS" ]; then
        echo "ERROR: GitHub device authorization timed out" >&2
        return 1
    fi

    # Step 3: Exchange GitHub token for Copilot token
    echo "→ Exchanging GitHub token for Copilot session..."
    CP_RESPONSE=$(curl -sf "https://api.github.com/copilot_internal/v2/token" \
        -H "Authorization: token ${GH_ACCESS}" \
        -H "Accept: application/json") || {
        echo "ERROR: Copilot token exchange failed. Is Copilot enabled for this account?" >&2
        return 1
    }

    CP_TK=$(echo "$CP_RESPONSE" | jq -r '.token // empty')
    CP_API=$(echo "$CP_RESPONSE" | jq -r '.endpoints.api // empty')

    if [ -z "$CP_TK" ]; then
        echo "ERROR: Failed to get Copilot token from response" >&2
        echo "$CP_RESPONSE" >&2
        return 1
    fi

    CP_BASE="${CP_API:-${COPILOT_DEFAULT_BASE_URL}}"

    # Cache only the Copilot session token — do NOT persist the GitHub access token
    # (it has read:user scope, never expires, and should only live in-memory)
    write_cache_provider copilot "$(jq -n \
        --arg token "$CP_TK" \
        --arg base_url "$CP_BASE" \
        '{token: $token, base_url: $base_url}')"

    echo "  Copilot credentials cached to ${TOKEN_CACHE}"

    export COPILOT_TOKEN="$CP_TK"
    export COPILOT_BASE_URL="$CP_BASE"
}

# Run device flows for providers that still need tokens
if [ -z "${COPILOT_TOKEN:-}" ] && [ "${COPILOT_ENABLED:-false}" != "false" ]; then
    run_copilot_device_flow || echo "  Skipping Copilot (device flow failed or declined)"
fi

# ── Summary ──────────────────────────────────────────────────────────
echo ""
echo "→ Configured providers:"
[ -n "${KIRO_REFRESH_TOKEN:-}" ] && echo "  - Kiro (AWS SSO)"
[ -n "${ANTHROPIC_API_KEY:-}" ] && echo "  - Anthropic (API key)"
[ -n "${OPENAI_API_KEY:-}" ] && echo "  - OpenAI (API key)"
[ -n "${COPILOT_TOKEN:-}" ] && echo "  - Copilot (GitHub)"
[ -n "${CUSTOM_PROVIDER_URL:-}" ] && echo "  - Custom (${CUSTOM_PROVIDER_URL})"
echo ""

echo "→ Starting Harbangan Gateway..."
exec /app/harbangan
