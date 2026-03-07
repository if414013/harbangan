# Messaging Pattern Templates

Ready-to-use message templates for common rkgw team communication scenarios. Use these structured formats for clear, actionable inter-agent messages.

## 1. Task Assignment

```
You've been assigned task #{id}: {subject}.

Owned files:
- {file1}
- {file2}

Key requirements:
- {requirement1}
- {requirement2}

Interface contract:
- Import {types} from {shared-file}
- Export {types} for {other-agent}

Let me know if you have questions or blockers.
```

**rkgw example** (scrum-master to rust-backend-engineer):
```
You've been assigned task #3: Add MCP health check endpoint.

Owned files:
- backend/src/mcp/client_manager.rs
- backend/src/mcp/mod.rs

Key requirements:
- Return health status for each registered MCP server
- Include last_seen timestamp and connection state

Interface contract:
- Export McpHealthStatus from mcp/mod.rs
- react-frontend-engineer will consume via /_ui/api/admin/mcp/clients/health

Let me know if you have questions or blockers.
```

## 2. Integration Point Notification

```
My side of the {interface-name} interface is complete.

Exported from {file}:
- {function/type 1}
- {function/type 2}

You can now import these in your owned files. The contract matches what we agreed on.
```

**rkgw example** (rust-backend-engineer to react-frontend-engineer):
```
My side of the guardrails-config interface is complete.

Exported from backend/src/web_ui/guardrails.rs:
- GET /_ui/api/guardrails/profiles -> Vec<GuardrailProfile>
- PUT /_ui/api/guardrails/profiles/{id} -> GuardrailProfile

You can now build the config UI in your owned files. The contract matches what we agreed on.
```

## 3. Blocker Report

```
I'm blocked on task #{id}: {subject}.

Blocker: {description of what's preventing progress}
Impact: {what can't be completed until this is resolved}

Options:
1. {option 1}
2. {option 2}

Waiting for your guidance.
```

**rkgw example** (react-frontend-engineer to scrum-master):
```
I'm blocked on task #5: MCP server management page.

Blocker: The backend endpoint GET /_ui/api/admin/mcp/clients returns a different
shape than the agreed contract — missing "transport_type" field.
Impact: Cannot render the server list table without transport info.

Options:
1. rust-backend-engineer adds transport_type to the response
2. I hardcode "unknown" and we fix later

Waiting for your guidance.
```

## 4. Task Completion Report

```
Task #{id} complete: {subject}

Changes made:
- {file1}: {what changed}
- {file2}: {what changed}

Integration notes:
- {any interface changes or considerations for other agents}

Ready for next assignment.
```

**rkgw example** (rust-backend-engineer to scrum-master):
```
Task #2 complete: Per-user Kiro token refresh

Changes made:
- backend/src/auth/mod.rs: Added auto-refresh 60s before expiry
- backend/src/web_ui/user_kiro.rs: New endpoint for manual token refresh
- backend/src/routes/mod.rs: Wired refresh into middleware chain

Integration notes:
- Token cache TTL changed from 5min to 4min — no frontend impact
- New endpoint POST /_ui/api/kiro/refresh available for react-frontend-engineer

Ready for next assignment.
```

## 5. Review Finding Summary

```
Review complete for {target} ({dimension}).

Summary:
- Critical: {count}
- High: {count}
- Medium: {count}
- Low: {count}

Top finding: {brief description of most important finding}

Full findings attached to task #{id}.
```

**rkgw example** (backend-qa to scrum-master):
```
Review complete for converters/openai_to_kiro.rs (correctness).

Summary:
- Critical: 1
- High: 0
- Medium: 2
- Low: 1

Top finding: Tool call arguments not escaped when content contains
double quotes — causes malformed JSON in Kiro request payload.

Full findings attached to task #7.
```

## 6. Investigation Report

```
Investigation complete for hypothesis: {hypothesis summary}

Verdict: {Confirmed | Falsified | Inconclusive}
Confidence: {High | Medium | Low}

Key evidence:
- {file:line}: {what was found}
- {file:line}: {what was found}

{If confirmed}: Recommended fix: {brief fix description}
{If falsified}: Contradicting evidence: {brief description}

Full report attached to task #{id}.
```

**rkgw example** (rust-backend-engineer to scrum-master):
```
Investigation complete for hypothesis: SSE stream drops on large responses
due to nginx proxy_buffering.

Verdict: Confirmed
Confidence: High

Key evidence:
- frontend/nginx.conf:42: proxy_buffering is set to "on" for /v1/* routes
- backend/src/streaming/mod.rs:187: Chunked responses exceed 8k default buffer

Recommended fix: Set proxy_buffering off for /v1/* location block.

Full report attached to task #12.
```

## 7. Shutdown Acknowledgment

```
Wrapping up. Current status:
- Task #{id}: {completed | in-progress}
- Files modified: {list}
- Pending work: {none | description}

Ready for shutdown.
```

## 8. General Guidance

```
Guidance for {agent-name} on {topic}:

Context: {why this matters}

Approach:
1. {step 1}
2. {step 2}
3. {step 3}

Constraints:
- {constraint 1}
- {constraint 2}

Refer to {reference doc or code path} for details.
```

**rkgw example** (scrum-master to devops-engineer):
```
Guidance for devops-engineer on cert renewal fix:

Context: Let's Encrypt certs expired because certbot container exits
after first renewal attempt instead of looping.

Approach:
1. Check init-certs.sh for the renewal loop logic
2. Verify docker-compose.yml certbot service has restart: unless-stopped
3. Test with --dry-run flag before applying

Constraints:
- Do not modify nginx.conf — that's frontend territory
- Keep the 12h renewal cycle interval

Refer to docker-compose.yml certbot service definition for details.
```
