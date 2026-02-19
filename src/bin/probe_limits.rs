//! Probe the actual context window and output token limits for each model
//! supported by the Kiro gateway.
//!
//! # Usage
//! ```bash
//! # Probe all models
//! cargo run --bin probe_limits --release
//!
//! # Probe a single model
//! cargo run --bin probe_limits --release -- --model claude-sonnet-4.6
//! ```
//!
//! Requires the gateway to be running locally. Configure via env vars:
//! - `GATEWAY_URL` - Gateway base URL (default: http://127.0.0.1:8000)
//! - `PROXY_API_KEY` - Gateway API key (required)

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Debug)]
enum Mode {
    SingleModel(String),
    AllModels,
}

impl Mode {
    fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            Some("--model") => {
                let model = args.next().context("--model requires a model ID")?;
                Ok(Self::SingleModel(model))
            }
            Some("--all-models") => Ok(Self::AllModels),
            Some(other) => anyhow::bail!("Unknown argument: {other}"),
            None => anyhow::bail!(
                "Usage:\n  probe_limits --model <model-id>\n  probe_limits --all-models"
            ),
        }
    }
}

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8000";
const PROBE_CONTENT: &str = "hello world ";
const OUTPUT_PROBE_PROMPT: &str =
    "Write a detailed essay about the history of computing. Be thorough and verbose. Do not stop until you have written at least 10000 words.";

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let mode = Mode::parse()?;
    let gateway_url =
        std::env::var("GATEWAY_URL").unwrap_or_else(|_| DEFAULT_GATEWAY_URL.to_string());
    let api_key = std::env::var("PROXY_API_KEY").context("PROXY_API_KEY env var required")?;

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    println!("Gateway: {gateway_url}");

    let models = match mode {
        Mode::SingleModel(model) => {
            println!("Probing model: {model}\n");
            vec![model]
        }
        Mode::AllModels => {
            println!("Fetching supported models...\n");
            let all = fetch_models(&client, &gateway_url, &api_key).await?;
            println!("Models to probe: {}\n", all.join(", "));
            all
        }
    };

    // Collect results first (progress goes to stderr), then print table to stdout
    let mut results = Vec::new();
    for model in &models {
        eprintln!("[{model}]");
        eprint!("  context window: ");
        let context_limit = probe_context_window(&client, &gateway_url, &api_key, model).await;
        eprintln!();
        eprint!("  output tokens:  ");
        let output_limit = probe_output_tokens(&client, &gateway_url, &api_key, model).await;
        eprintln!();
        results.push((model, context_limit, output_limit));
    }

    eprintln!();
    println!(
        "{:<32} | {:>18} | Max output tokens",
        "Model", "Context (tokens)"
    );
    println!("{}", "-".repeat(72));

    for (model, context_limit, output_limit) in &results {
        let ctx_str = match context_limit {
            Ok(n) => format!("~{}K", n / 1000),
            Err(e) => format!("err: {e}"),
        };
        let out_str = match output_limit {
            Ok(Some(n)) => format!("~{}K tokens", n / 1000),
            Ok(None) => "unknown (see tip below)".to_string(),
            Err(e) => format!("err: {e}"),
        };

        println!("{model:<32} | {ctx_str:>18} | {out_str}");
    }

    println!("\nTip: use Context (tokens) for `contextLength` in your OpenCode provider config.");
    println!("     If Max output tokens shows 'unknown', the model stopped early before hitting");
    println!("     its cap. Restart the gateway with FAKE_REASONING=false and re-run.");
    Ok(())
}

/// Fetch all model IDs from the gateway's /v1/models endpoint.
async fn fetch_models(client: &Client, base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let resp = client
        .get(format!("{base_url}/v1/models"))
        .bearer_auth(api_key)
        .send()
        .await?
        .json::<Value>()
        .await?;

    let models = resp["data"]
        .as_array()
        .context("Expected data array in /v1/models response")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .filter(|id| id.starts_with("claude-"))
        .collect();

    Ok(models)
}

/// Binary search for the context window limit.
/// Sends increasing payloads and reads `prompt_tokens` from the usage metadata.
/// Returns the highest `prompt_tokens` value that succeeded.
async fn probe_context_window(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<u64> {
    let mut lo_chars: usize = 100_000;
    let mut hi_chars: usize = 5_000_000;
    // First check if even lo_chars works
    let mut last_good_tokens: u64 =
        match send_probe(client, base_url, api_key, model, lo_chars).await {
            Ok(usage) => usage.prompt_tokens,
            Err(_) => return Err(anyhow::anyhow!("model unavailable")),
        };

    // Check if hi_chars fails (if not, context window is very large)
    if send_probe(client, base_url, api_key, model, hi_chars)
        .await
        .is_ok()
    {
        return Ok(last_good_tokens); // Can't find ceiling, report last known
    }

    // Binary search
    while hi_chars - lo_chars > 20_000 {
        let mid = (lo_chars + hi_chars) / 2;
        eprint!(" [{:.0}K chars]", mid as f64 / 1000.0);
        match send_probe(client, base_url, api_key, model, mid).await {
            Ok(usage) => {
                last_good_tokens = usage.prompt_tokens;
                lo_chars = mid;
                eprint!("✓");
            }
            Err(_) => {
                hi_chars = mid;
                eprint!("✗");
            }
        }
    }

    Ok(last_good_tokens)
}

/// Try to find the output token cap by forcing `finish_reason=length`.
/// Returns `Some(n)` if we hit the cap, `None` if the model always stops early.
async fn probe_output_tokens(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Result<Option<u64>> {
    // First confirm finish=length is achievable at a small cap.
    // If the model stops early even at 200 tokens, it will stop early at any cap —
    // no point trying larger values.
    eprint!(" [feasibility check]");
    let result = send_chat(client, base_url, api_key, model, OUTPUT_PROBE_PROMPT, 200).await?;
    if result.finish_reason.as_deref() != Some("length") {
        // If completion_tokens >> max_tokens, thinking is consuming the budget
        let thinking_likely = result.completion_tokens > 150;
        if thinking_likely {
            eprint!(" (thinking mode consuming budget — restart with FAKE_REASONING=false)");
        } else {
            eprint!(" (model stopped early — try a different prompt or disable thinking)");
        }
        return Ok(None);
    }
    eprint!("✓");

    // Binary search for the actual ceiling
    let mut lo: u64 = 200;
    let mut hi: u64 = 65536;
    let mut last_good = lo;

    // Confirm hi fails (model is capped below hi)
    eprint!(" [out:{hi}]");
    let hi_result = send_chat(client, base_url, api_key, model, OUTPUT_PROBE_PROMPT, hi).await?;
    if hi_result.finish_reason.as_deref() == Some("length") {
        // Still hitting length at 65536 — report that and stop
        eprint!("✓ (at ceiling)");
        return Ok(Some(hi_result.completion_tokens));
    }
    eprint!("✗");

    while hi - lo > 256 {
        let mid = (lo + hi) / 2;
        eprint!(" [out:{mid}]");
        let r = send_chat(client, base_url, api_key, model, OUTPUT_PROBE_PROMPT, mid).await?;
        if r.finish_reason.as_deref() == Some("length") {
            last_good = r.completion_tokens;
            lo = mid;
            eprint!("✓");
        } else {
            hi = mid;
            eprint!("✗");
        }
    }

    Ok(Some(last_good))
}

// === HTTP helpers ===

struct ProbeUsage {
    prompt_tokens: u64,
}

struct ChatResult {
    finish_reason: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
}

async fn send_probe(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    approx_chars: usize,
) -> Result<ProbeUsage> {
    let content = PROBE_CONTENT.repeat(approx_chars / PROBE_CONTENT.len() + 1);
    let content = &content[..approx_chars];
    let prompt = format!("{content}\n\nSay ok");

    let result = send_chat(client, base_url, api_key, model, &prompt, 5).await?;
    Ok(ProbeUsage {
        prompt_tokens: result.prompt_tokens,
    })
}

async fn send_chat(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    prompt: &str,
    max_tokens: u64,
) -> Result<ChatResult> {
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": max_tokens,
        "stream": true,
        "stream_options": {"include_usage": true}
    });

    let resp = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("HTTP {status}: {text}"));
    }

    let text = resp.text().await?;
    let mut prompt_tokens: u64 = 0;
    let mut completion_tokens: u64 = 0;
    let mut finish_reason: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("data:") || line == "data: [DONE]" {
            continue;
        }
        let json_str = line.trim_start_matches("data:").trim();
        let Ok(chunk) = serde_json::from_str::<Value>(json_str) else {
            continue;
        };

        if let Some(usage) = chunk.get("usage") {
            prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
            completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
        }

        if let Some(choices) = chunk["choices"].as_array() {
            for choice in choices {
                if let Some(fr) = choice["finish_reason"].as_str() {
                    finish_reason = Some(fr.to_string());
                }
            }
        }
    }

    Ok(ChatResult {
        finish_reason,
        prompt_tokens,
        completion_tokens,
    })
}
