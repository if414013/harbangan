#!/bin/sh
set -e

# Proxy-only entrypoint for harbangan backend.
# Runs the AWS SSO OIDC device code flow if no refresh token exists,
# caches credentials to /data/tokens.json for reuse across restarts,
# then launches the gateway binary with the obtained credentials.

TOKEN_CACHE="/data/tokens.json"
OIDC_REGION="${KIRO_SSO_REGION:-${KIRO_REGION:-us-east-1}}"
OIDC_BASE="https://oidc.${OIDC_REGION}.amazonaws.com"

# ── Validate ─────────────────────────────────────────────────────────
if [ -z "$PROXY_API_KEY" ]; then
    echo "ERROR: PROXY_API_KEY is required" >&2
    exit 1
fi

# ── Try loading cached tokens ────────────────────────────────────────
load_cached_tokens() {
    if [ -f "$TOKEN_CACHE" ]; then
        CACHED_REFRESH=$(jq -r '.refresh_token // empty' "$TOKEN_CACHE" 2>/dev/null)
        CACHED_CLIENT_ID=$(jq -r '.client_id // empty' "$TOKEN_CACHE" 2>/dev/null)
        CACHED_CLIENT_SECRET=$(jq -r '.client_secret // empty' "$TOKEN_CACHE" 2>/dev/null)
        if [ -n "$CACHED_REFRESH" ] && [ -n "$CACHED_CLIENT_ID" ] && [ -n "$CACHED_CLIENT_SECRET" ]; then
            return 0
        fi
    fi
    return 1
}

# ── Validate cached tokens with a test refresh ──────────────────────
validate_cached_tokens() {
    echo "→ Validating cached credentials..."
    VALIDATE_RESPONSE=$(curl -s -X POST "${OIDC_BASE}/token" \
        -H "Content-Type: application/json" \
        -d "{\"grantType\":\"refresh_token\",\"clientId\":\"${CACHED_CLIENT_ID}\",\"clientSecret\":\"${CACHED_CLIENT_SECRET}\",\"refreshToken\":\"${CACHED_REFRESH}\"}")

    VALIDATED_ACCESS=$(echo "$VALIDATE_RESPONSE" | jq -r '.accessToken // empty')
    if [ -n "$VALIDATED_ACCESS" ]; then
        # Update refresh token if rotated
        NEW_REFRESH=$(echo "$VALIDATE_RESPONSE" | jq -r '.refreshToken // empty')
        if [ -n "$NEW_REFRESH" ]; then
            CACHED_REFRESH="$NEW_REFRESH"
            save_tokens "$CACHED_REFRESH" "$CACHED_CLIENT_ID" "$CACHED_CLIENT_SECRET"
        fi
        echo "  Cached credentials valid"
        return 0
    fi
    echo "  Cached credentials expired or invalid"
    return 1
}

# ── Save tokens to cache ────────────────────────────────────────────
save_tokens() {
    mkdir -p "$(dirname "$TOKEN_CACHE")"
    cat > "$TOKEN_CACHE" <<TOKEOF
{"refresh_token":"$1","client_id":"$2","client_secret":"$3"}
TOKEOF
    chmod 600 "$TOKEN_CACHE"
}

# ── Resolve credentials ─────────────────────────────────────────────
if [ -z "$KIRO_REFRESH_TOKEN" ]; then
    # Try cache first
    if load_cached_tokens && validate_cached_tokens; then
        export KIRO_REFRESH_TOKEN="$CACHED_REFRESH"
        export KIRO_CLIENT_ID="$CACHED_CLIENT_ID"
        export KIRO_CLIENT_SECRET="$CACHED_CLIENT_SECRET"
    else
        # Cache miss or stale — run device code flow
        rm -f "$TOKEN_CACHE"

        echo ""
        echo "┌─────────────────────────────────────────────────────────┐"
        echo "│  Kiro Gateway — Proxy-Only Mode                         │"
        echo "├─────────────────────────────────────────────────────────┤"
        echo "│  KIRO_REGION:    ${KIRO_REGION:-us-east-1}"
        echo "│  OIDC_REGION:    ${OIDC_REGION}"
        if [ -n "$KIRO_SSO_URL" ]; then
            echo "│  KIRO_SSO_URL:   $KIRO_SSO_URL"
            echo "│  Login mode:     Identity Center (pro)"
        else
            echo "│  Login mode:     Builder ID (free)"
        fi
        echo "└─────────────────────────────────────────────────────────┘"
        echo ""

        # ── Step 1: Register OIDC client ─────────────────────────────
        echo "→ Registering OIDC client at ${OIDC_BASE}..."

        REGISTER_BODY="{\"clientName\":\"harbangan-proxy\",\"clientType\":\"public\",\"scopes\":[\"codewhisperer:completions\",\"codewhisperer:analysis\",\"codewhisperer:conversations\"],\"grantTypes\":[\"urn:ietf:params:oauth:grant-type:device_code\",\"refresh_token\"]"

        if [ -n "$KIRO_SSO_URL" ]; then
            REGISTER_BODY="${REGISTER_BODY},\"issuerUrl\":\"${KIRO_SSO_URL}\""
        fi
        REGISTER_BODY="${REGISTER_BODY}}"

        REG_RESPONSE=$(curl -sf -X POST "${OIDC_BASE}/client/register" \
            -H "Content-Type: application/json" \
            -d "$REGISTER_BODY") || {
            echo "ERROR: OIDC client registration failed" >&2
            exit 1
        }

        CLIENT_ID=$(echo "$REG_RESPONSE" | jq -r '.clientId')
        CLIENT_SECRET=$(echo "$REG_RESPONSE" | jq -r '.clientSecret')

        if [ -z "$CLIENT_ID" ] || [ "$CLIENT_ID" = "null" ]; then
            echo "ERROR: Failed to parse client registration response" >&2
            echo "$REG_RESPONSE" >&2
            exit 1
        fi

        echo "  Client registered (${CLIENT_ID%${CLIENT_ID#????????}}...)"

        # ── Step 2: Start device authorization ───────────────────────
        START_URL="${KIRO_SSO_URL:-https://view.awsapps.com/start}"

        DEVICE_RESPONSE=$(curl -sf -X POST "${OIDC_BASE}/device_authorization" \
            -H "Content-Type: application/json" \
            -d "{\"clientId\":\"${CLIENT_ID}\",\"clientSecret\":\"${CLIENT_SECRET}\",\"startUrl\":\"${START_URL}\"}") || {
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
        echo "║  Open this URL in your browser to authorize:             ║"
        echo "║                                                          ║"
        echo "║  $VERIFY_URL"
        echo "║                                                          ║"
        echo "║  User code: $USER_CODE"
        echo "╚═══════════════════════════════════════════════════════════╝"
        echo ""
        echo "→ Waiting for authorization (expires in ${EXPIRES_IN}s)..."

        # ── Step 3: Poll for token ───────────────────────────────────
        ELAPSED=0
        while [ "$ELAPSED" -lt "$EXPIRES_IN" ]; do
            sleep "$INTERVAL"
            ELAPSED=$((ELAPSED + INTERVAL))

            TOKEN_RESPONSE=$(curl -s -X POST "${OIDC_BASE}/token" \
                -H "Content-Type: application/json" \
                -d "{\"grantType\":\"urn:ietf:params:oauth:grant-type:device_code\",\"clientId\":\"${CLIENT_ID}\",\"clientSecret\":\"${CLIENT_SECRET}\",\"deviceCode\":\"${DEVICE_CODE}\"}")

            # Check for success (has access_token)
            ACCESS_TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.accessToken // empty')
            if [ -n "$ACCESS_TOKEN" ]; then
                REFRESH_TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.refreshToken // empty')
                echo ""
                echo "✅ Authorization successful!"
                echo ""
                break
            fi

            # Check for slow_down
            if echo "$TOKEN_RESPONSE" | grep -q "slow_down"; then
                INTERVAL=$((INTERVAL + 1))
                continue
            fi

            # Check for authorization_pending (keep polling)
            if echo "$TOKEN_RESPONSE" | grep -q "authorization_pending"; then
                continue
            fi

            # Unexpected error
            echo "ERROR: Token polling failed:" >&2
            echo "$TOKEN_RESPONSE" >&2
            exit 1
        done

        if [ -z "$REFRESH_TOKEN" ]; then
            echo "ERROR: Device authorization timed out. Please restart and try again." >&2
            exit 1
        fi

        # Cache and export
        save_tokens "$REFRESH_TOKEN" "$CLIENT_ID" "$CLIENT_SECRET"
        echo "  Credentials cached to ${TOKEN_CACHE}"

        export KIRO_REFRESH_TOKEN="$REFRESH_TOKEN"
        export KIRO_CLIENT_ID="$CLIENT_ID"
        export KIRO_CLIENT_SECRET="$CLIENT_SECRET"
    fi
fi

echo "→ Starting Kiro Gateway..."
exec /app/harbangan
