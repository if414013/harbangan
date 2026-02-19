# Probing Model Limits

`probe_limits` is a binary that empirically tests the context window and output token limits for each model supported by the gateway. Use it to determine the correct values for your OpenCode provider config.

## Prerequisites

The gateway must be running locally before you run this tool.

## Usage

```bash
# Probe a single model
cargo run --bin probe_limits --release -- --model claude-sonnet-4.6

# Probe all claude-* models
cargo run --bin probe_limits --release -- --all-models
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PROXY_API_KEY` | *(required)* | Gateway API key |
| `GATEWAY_URL` | `http://127.0.0.1:8000` | Gateway base URL |

These are read from `.env` automatically if present.

## Output

```
Gateway: http://127.0.0.1:8000
Probing model: claude-sonnet-4.6

[claude-sonnet-4.6]
  context window:  [2550K chars]✗ [1325K chars]✗ ... done.
  output tokens:   [feasibility check] (thinking mode consuming budget — restart with FAKE_REASONING=false)

Model                            |   Context (tokens) | Max output tokens
--------------------------------------------------------------------------
claude-sonnet-4.6                |             ~195K  | unknown (thinking mode consuming budget — restart with FAKE_REASONING=false)
```

**Context (tokens)** — the highest `prompt_tokens` value that succeeded, read directly from the gateway's usage metadata. Use this for `contextLength` in your OpenCode config.

**Max output tokens** — if the model hit `finish_reason=length`, shows the actual `completion_tokens` at the cap. Otherwise shows `unknown` with a reason:
- `thinking mode consuming budget` — thinking tokens are eating `max_tokens`, restart with `FAKE_REASONING=false`
- `model stopped early` — thinking is off but the model still finished before hitting the cap; use Anthropic's documented limit as a baseline

## OpenCode Config

Map the results to your provider's `models` block:

```json
"claude-sonnet-4.6": {
  "name": "Claude Sonnet 4.6",
  "limit": {
    "context": 198000,
    "output": 8192
  }
}
```

## Why the Model Stops Early

When the output cap shows `model stops early`, it means every request returned `finish_reason=stop` — the model decided it was done before hitting `max_tokens`. This is normal behavior; models don't generate indefinitely just because you set a high cap.

There are two distinct causes:

**1. Thinking mode is on (most common)**

When `FAKE_REASONING=true` (the default), the model spends most of its `max_tokens` budget on internal reasoning before writing a single word of output. The text response is short, the model finishes naturally, and `finish_reason=stop` every time.

Fix: restart the gateway with thinking disabled before probing output limits:

```bash
FAKE_REASONING=false cargo run --release
```

Then re-run the probe. You should start seeing `finish_reason=length` for small `max_tokens` values.

**2. The prompt doesn't require long output**

Even with thinking off, if the prompt has a natural stopping point (e.g. "say hi"), the model finishes early. The probe uses a code generation prompt to encourage longer output, but some models still summarize instead of generating exhaustively.

If you need a definitive output cap, use a prompt that forces continuation — for example, prefill the assistant turn mid-sentence so the model has no natural place to stop.

**What `model stops early` means for your config**

It doesn't mean the model has no output limit — it means the probe couldn't find it empirically. In this case, use Anthropic's documented limit as a baseline:

| Model family | Standard max output |
|---|---|
| Claude 3.x | 4096 tokens |
| Claude 4.x (Haiku, Sonnet) | 8192 tokens |
| Claude 4.x (Opus) | 8192 tokens |

Set `output` in your OpenCode config to one of these values. Kiro will silently clamp requests that exceed the real limit — it won't return an error.

## Notes

- **Context probe accuracy**: The binary search uses character count as a proxy for tokens (~4 chars/token). The reported token count comes from the gateway's tiktoken estimate, not Kiro's tokenizer directly.
- **`auto` model**: Skipped by default since it's a routing alias, not a real model with its own limits.
