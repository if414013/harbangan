# Interactive Slack-Driven Workflows

## Context

The orchestrator currently runs tasks autonomously — user fires a slash command, task runs to completion, result posted to Slack. No way to interact mid-execution. The user wants two interactive flows:

1. **Planning**: `/harbangan plan ...` → agent explores → asks user questions in Slack thread → user answers → plan finalized → user confirms → implement task auto-created
2. **PR Review**: GitHub webhook triggers on PR open → agent reviews → posts to GitHub + Slack → if confident, auto-creates fix task → if uncertain, asks user in Slack first

## Architecture

```
Slack (Socket Mode WS)              GitHub Webhooks
        │                                  │
        ▼                                  ▼
┌─────────────────┐              ┌──────────────────┐
│  SlackService    │              │ GitHub Webhook    │
│  (@slack/bolt)   │              │ Handler           │
│  - /harbangan cmd│              │ - PR open/sync    │
│  - thread replies│              │ - signature verify │
└────────┬────────┘              └────────┬──────────┘
         │                                │
         ▼                                ▼
┌─────────────────────────────────────────────────────┐
│              InteractionBridge                       │
│  Map<taskId → { channelId, threadTs, resolvers[] }> │
│                                                      │
│  askUser(taskId, question) → Promise<string>         │
│    1. Posts question to Slack thread                  │
│    2. Creates deferred Promise + timeout timer       │
│    3. BLOCKS until user replies or timeout            │
│                                                      │
│  handleReply(threadTs, text)                         │
│    → resolves oldest pending Promise (FIFO)          │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│           ask_user MCP Tool                          │
│  Created via createSdkMcpServer() + tool()           │
│  Registered per-workflow in query() options           │
│                                                      │
│  Agent calls: mcp__slack__ask_user({ question })     │
│  Handler calls: bridge.askUser(taskId, question)     │
│  Returns user's text response to agent               │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────┐
│           Workflows (plan.ts, pr-review.ts)          │
│  Pass mcpServers to executeAgent()                   │
│  Agent autonomously decides when to ask_user         │
└─────────────────────────────────────────────────────┘
```

## Scenario 1: Interactive Planning

```
User in Slack                    Orchestrator                     Agent
     │                              │                               │
     │  /harbangan plan Add         │                               │
     │  rate limiting               │                               │
     │─────────────────────────────>│                               │
     │                              │  Create plan task             │
     │  "🔍 Planning started..."   │  Post parent msg → get ts     │
     │<─────────────────────────────│  Register thread in bridge    │
     │                              │  Start plan workflow          │
     │                              │─────────────────────────────>│
     │                              │                               │  Explore code
     │                              │                               │  ...
     │                              │                               │  Calls ask_user
     │                              │  bridge.askUser()             │
     │  "Should this apply to       │<─────────────────────────────│
     │   both /v1/chat and          │                               │
     │   /v1/messages?"             │  [BLOCKED]                    │
     │<─────────────────────────────│                               │
     │                              │                               │
     │  "Both endpoints"            │                               │
     │─────────────────────────────>│  Resolve Promise              │
     │                              │─────────────────────────────>│
     │                              │                               │  Continue planning
     │                              │                               │  Calls ask_user
     │  "Here's the plan.           │                               │
     │   Proceed? (yes/no)"         │                               │
     │<─────────────────────────────│                               │
     │                              │                               │
     │  "yes"                       │                               │
     │─────────────────────────────>│                               │
     │                              │─────────────────────────────>│  Returns plan
     │                              │                               │
     │                              │  Auto-create implement task   │
     │  "✅ Implementation task     │  with plan_task_id            │
     │   #147 created"              │                               │
     │<─────────────────────────────│                               │
```

## Scenario 2: PR Review

```
GitHub                     Orchestrator                    Slack
  │                              │                           │
  │  PR #145 opened              │                           │
  │  (webhook POST)              │                           │
  │─────────────────────────────>│                           │
  │                              │  Verify signature         │
  │                              │  Create pr-review task    │
  │                              │  Post "Reviewing PR #145" │
  │                              │──────────────────────────>│
  │                              │                           │
  │  Agent posts review          │  Review runs...           │
  │  comments on PR              │                           │
  │<─────────────────────────────│                           │
  │                              │                           │
  │                              │  IF confident + issues:   │
  │                              │  Auto-create fix task     │
  │                              │  "🔧 Fix task #148"      │
  │                              │──────────────────────────>│
  │                              │                           │
  │                              │  IF uncertain:            │
  │                              │  "I found X but I'm not   │
  │                              │   sure. Create fix task?" │
  │                              │──────────────────────────>│
  │                              │                           │  User: "yes"
  │                              │<──────────────────────────│
  │                              │  Create fix task          │
  │                              │                           │
  │                              │  IF clean (APPROVE):      │
  │                              │  "✅ PR #145 approved"    │
  │                              │──────────────────────────>│
```

## New Files

| File | LOC | Purpose |
|------|-----|---------|
| `src/slack/service.ts` | ~120 | `SlackService` — wraps `@slack/bolt` App with Socket Mode, handles commands + thread replies |
| `src/slack/interaction-bridge.ts` | ~100 | `InteractionBridge` — maps tasks↔threads, manages blocking Promises with timeout |
| `src/slack/ask-user-tool.ts` | ~40 | Creates `ask_user` MCP tool via `createSdkMcpServer()` |
| `src/slack/types.ts` | ~20 | Type definitions |
| `src/slack/blocks.ts` | ~80 | Block Kit message builders |
| `src/api/github-webhook.ts` | ~60 | GitHub webhook handler (signature verify, PR event → task) |
| `tests/interaction-bridge.test.ts` | ~100 | Bridge unit tests |
| `tests/ask-user-tool.test.ts` | ~50 | MCP tool unit tests |

## Modified Files

| File | Change |
|------|--------|
| `src/config.ts` | Add `slackAppToken`, `githubWebhookSecret`, `askUserTimeoutMs` |
| `src/store/types.ts` | Add `slack_channel_id`, `slack_thread_ts` to `TaskInput` |
| `src/workflows/base.ts` | Add `mcpServers` param to `executeAgent()`, add `interactionBridge`/`slackService` to `WorkflowContext` |
| `src/workflows/plan.ts` | Create ask_user MCP server when Slack is configured, update prompt, auto-create implement task on "yes" |
| `src/workflows/pr-review.ts` | Post review to Slack, auto-create fix task (confident) or ask user (uncertain) |
| `src/index.ts` | Init SlackService + InteractionBridge, pass to WorkflowContext, lifecycle management |
| `src/api/router.ts` | Add `POST /api/v1/github/webhook` route, remove inline Slack webhook handler |
| `src/api/validators.ts` | Add `githubWebhookSchema` |
| `.env.example` | Add `SLACK_APP_TOKEN`, `GITHUB_WEBHOOK_SECRET`, `ASK_USER_TIMEOUT_MS` |
| `docker-compose.yml` | Add new env vars |

## Key Design Decisions

**Socket Mode for Slack** — No public URL needed. The orchestrator initiates a WebSocket connection to Slack. `@slack/bolt` (already in package.json) supports it natively. Requires a `SLACK_APP_TOKEN` (xapp- token).

**InteractionBridge as central coordinator** — Decouples Slack routing from workflow logic. One Map holds task→thread mappings and pending Promise resolvers. Thread replies resolve the oldest pending Promise (FIFO).

**ask_user via createSdkMcpServer()** — The Agent SDK supports registering in-process MCP tools with async handlers. The tool handler calls `bridge.askUser()` which blocks until the user replies. The agent sees it as a normal tool call. No prompt engineering hacks.

**Timeout: 30 min default** — Configurable via `ASK_USER_TIMEOUT_MS`. On timeout, the Promise rejects. The agent receives an error and can decide to proceed with assumptions or abort.

**PR review loop prevention** — When auto-creating fix tasks from reviews, tag with `auto_fix: true` in task input. Skip auto-fix for PRs created from auto_fix tasks.

## Implementation Waves

### Wave 1 — Foundation (InteractionBridge + SlackService)

New: `src/slack/types.ts`, `src/slack/interaction-bridge.ts`, `src/slack/service.ts`, `src/slack/blocks.ts`
Modified: `src/config.ts`
Tests: `tests/interaction-bridge.test.ts`

The bridge and Slack service work independently — testable without touching workflows.

### Wave 2 — ask_user MCP Tool

New: `src/slack/ask-user-tool.ts`
Modified: `src/workflows/base.ts` (add `mcpServers` param + WorkflowContext fields)
Tests: `tests/ask-user-tool.test.ts`

### Wave 3 — Workflow Integration

Modified: `src/store/types.ts`, `src/workflows/plan.ts`, `src/workflows/pr-review.ts`, `src/index.ts`, `src/api/router.ts`

Wire everything together. Plan workflow gets interactive ask_user. PR review gets Slack summary + auto-fix logic.

### Wave 4 — GitHub Webhook

New: `src/api/github-webhook.ts`
Modified: `src/api/router.ts`, `src/api/validators.ts`

Standalone — receives PR events, creates review tasks.

### Wave 5 — Config + Deploy

Modified: `.env.example`, `docker-compose.yml`

## Slack App Setup (Manual Prerequisites)

1. Create Slack App at api.slack.com/apps
2. Enable Socket Mode → get `SLACK_APP_TOKEN` (xapp-)
3. Add Bot Token Scopes: `chat:write`, `commands`, `channels:history`
4. Create Slash Command: `/harbangan`
5. Enable Event Subscriptions (Socket Mode): `message.channels`, `message.groups`
6. Install to workspace → get `SLACK_BOT_TOKEN` (xoxb-)

## GitHub Webhook Setup (Manual Prerequisites)

1. Repo Settings → Webhooks → Add webhook
2. Payload URL: `https://<orchestrator-host>/api/v1/github/webhook`
3. Content type: `application/json`
4. Secret: set as `GITHUB_WEBHOOK_SECRET`
5. Events: select "Pull requests"

## Verification

1. `npx tsc --noEmit` — zero errors after each wave
2. `npx vitest run` — all tests pass
3. Set `SLACK_APP_TOKEN` + `SLACK_BOT_TOKEN` → verify Socket Mode connects
4. `/harbangan plan test feature` → verify thread created, agent asks questions, user can answer
5. Reply in thread → verify agent receives answer and continues
6. Agent finishes plan, asks "proceed?" → user says "yes" → implement task created
7. Open a PR → verify webhook creates pr-review task → review posted to GitHub + Slack
8. PR review with confident issues → verify fix task auto-created
9. PR review with uncertain findings → verify agent asks user in Slack before creating fix task
10. Test 30-min timeout → verify agent gets error and handles gracefully
