# provider-oauth-relay_20260307: Provider OAuth Relay + Profile Page Merge

**Type**: feature
**Created**: 2026-03-07
**Preset**: fullstack + backend-qa
**Services**: backend, frontend, backend-qa, frontend-qa

## Problem Statement

The current multi-provider implementation uses static API keys entered manually by users. This is insecure (keys stored in plaintext), poor UX (users must find and paste keys), and doesn't support token refresh. The OAuth relay pattern replaces API key input with a browser-based OAuth flow that reuses CLIProxyAPI's public client IDs, stores per-user tokens in PostgreSQL with automatic refresh, and consolidates the standalone `/providers` page into the existing `/profile` page where it belongs alongside Kiro tokens and API keys.

## User Story

As a gateway user, I want to connect my Anthropic/Gemini/OpenAI accounts via OAuth so that the gateway routes requests to my preferred provider without storing static API keys.

## Acceptance Criteria

1. Users can connect Anthropic/Gemini/OpenAI via OAuth relay from Profile page
2. Relay script runs on user's machine, captures OAuth callback, relays code to rkgw
3. Connected providers route model requests (claude-* -> Anthropic, gemini-* -> Gemini, gpt-* -> OpenAI)
4. Token refresh is automatic and mutex-locked per user+provider
5. Standalone /providers page and all API-key-based flows are fully removed

## Scope Boundaries

**Out of scope:**
1. Provider-specific model listing (use existing Kiro model list)
2. Admin-level provider management (this is per-user only)
3. Proxy-only mode support (OAuth requires DB)
4. Provider API rate limiting or quota tracking
5. Automatic provider selection/load balancing

## Dependencies

- Completed `multi-provider-support_20260307` track (registry, provider routing, AppState fields)
- Existing Google SSO PKCE patterns in `google_auth.rs` as reference
- Detailed implementation spec: `.claude/draft/provider-oauth-relay-plan.md`
