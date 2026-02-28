use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};

mod auth;
mod cache;
mod config;
mod converters;
mod dashboard;
mod error;
mod http_client;
mod metrics;
mod middleware;
mod models;
mod resolver;
mod routes;
mod streaming;
mod thinking_parser;
mod truncation;
mod tls;
mod tokenizer;
mod utils;

#[tokio::main]
async fn main() -> Result<()> {
    // Install the rustls crypto provider before any TLS operations.
    // Both `ring` and `aws-lc-rs` can end up compiled via transitive deps;
    // an explicit install prevents the runtime auto-detection panic.
    // If a provider is already installed, that's fine - we can continue.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Check if interactive setup is needed (no .env and missing required values)
    if config::needs_interactive_setup() {
        let interactive_config = config::run_interactive_setup()?;

        // Set environment variables from interactive config so Config::load() can use them
        std::env::set_var("PROXY_API_KEY", &interactive_config.proxy_api_key);
        std::env::set_var("KIRO_CLI_DB_FILE", &interactive_config.kiro_cli_db_file);
        std::env::set_var("KIRO_REGION", &interactive_config.kiro_region);
        std::env::set_var("SERVER_PORT", &interactive_config.server_port);
    }

    // Load configuration first (for log level)
    let config = config::Config::load()?;
    config.validate()?;

    let log_buffer = Arc::new(Mutex::new(VecDeque::new()));

    let log_level = config.log_level.to_lowercase();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_level));

    if config.dashboard {
        use tracing_subscriber::prelude::*;

        let dashboard_layer = dashboard::log_layer::DashboardLayer::new(Arc::clone(&log_buffer));

        tracing_subscriber::registry()
            .with(env_filter)
            .with(dashboard_layer)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true)
            .init();
    }

    tracing::info!("🚀 Kiro Gateway starting...");
    tracing::info!(
        "Server configured: {}:{}",
        config.server_host,
        config.server_port
    );
    tracing::debug!("Debug mode: {:?}", config.debug_mode);

    // Log auto-enable of TLS (must happen after tracing is initialized)
    if config.is_tls_active() && !config.tls_enabled {
        tracing::info!("TLS certificate and key provided — automatically enabling HTTPS.");
    }

    // Initialize authentication manager
    tracing::info!("Initializing authentication...");
    let auth_manager = Arc::new(auth::AuthManager::new(
        config.kiro_cli_db_file.clone(),
        config.token_refresh_threshold,
    )?);

    // Test authentication by getting a token
    match auth_manager.get_access_token().await {
        Ok(token) => {
            tracing::info!(
                "✅ Authentication successful (token: {}...)",
                &token[..20.min(token.len())]
            );
        }
        Err(e) => {
            tracing::error!("❌ Authentication failed: {}", e);
            tracing::warn!(
                "Server will start but API requests will fail without valid credentials"
            );
        }
    }

    // Initialize HTTP client
    let http_client = Arc::new(http_client::KiroHttpClient::new(
        auth_manager.clone(),
        config.http_max_connections,
        config.http_connect_timeout,
        config.http_request_timeout,
        config.http_max_retries,
    )?);
    tracing::info!("✅ HTTP client initialized with connection pooling");

    // Initialize model cache
    tracing::info!("Initializing model cache...");
    let model_cache = cache::ModelCache::new(3600); // 1 hour TTL

    // Load models from Kiro API at startup - fail fast on errors
    tracing::info!("Loading models from Kiro API...");
    let models = match load_models_from_kiro(&http_client, &auth_manager, &config).await {
        Ok(models) => models,
        Err(e) => {
            tracing::error!("❌ Failed to load models from Kiro API: {}", e);
            tracing::error!("");
            tracing::error!("🔧 Troubleshooting steps:");
            tracing::error!("   1. Check your network connection");
            tracing::error!("   2. Verify your Kiro CLI credentials are valid");
            tracing::error!("   3. Try logging out and logging back in:");
            tracing::error!("");
            tracing::error!("      kiro-cli logout");
            tracing::error!("      kiro-cli login");
            tracing::error!("");
            anyhow::bail!("Startup failed: Unable to connect to CodeWhisperer API");
        }
    };

    tracing::info!("📊 Models from Kiro API:");
    for model in &models {
        tracing::info!(
            "{}",
            serde_json::to_string_pretty(model).unwrap_or_default()
        );
    }

    model_cache.update(models);
    tracing::info!(
        "✅ Loaded {} models from Kiro API",
        model_cache.get_all_model_ids().len()
    );

    // Add hidden models to cache
    add_hidden_models(&model_cache);
    tracing::info!("✅ Added hidden models to cache");

    let resolver =
        resolver::ModelResolver::new(model_cache.clone(), std::collections::HashMap::new());
    tracing::info!("✅ Model resolver initialized");

    let metrics = Arc::new(metrics::MetricsCollector::new());
    tracing::info!("✅ Metrics collector initialized");

    let app_state = routes::AppState {
        proxy_api_key: config.proxy_api_key.clone(),
        model_cache: model_cache.clone(),
        auth_manager: auth_manager.clone(),
        http_client: http_client.clone(),
        resolver,
        config: Arc::new(config.clone()),
        metrics: Arc::clone(&metrics),
    };

    let app = build_app(app_state);

    // Use tuple form for lookup_host to properly handle IPv6 addresses like ::1
    let mut resolved_addrs =
        tokio::net::lookup_host((config.server_host.as_str(), config.server_port))
            .await
            .with_context(|| {
                format!(
                    "Failed to resolve server address '{}:{}'",
                    config.server_host, config.server_port
                )
            })?;
    let sock_addr: std::net::SocketAddr = resolved_addrs
        .next()
        .context("No resolved socket addresses for configured server host")?;
    let protocol = if config.is_tls_active() {
        "https"
    } else {
        "http"
    };

    // Build optional TLS configuration
    let rustls_config = if config.is_tls_active() {
        let tls_cfg = tls::TlsConfig {
            cert_path: config.tls_cert_path.clone(),
            key_path: config.tls_key_path.clone(),
        };
        let rc = tls_cfg.build_rustls_config().await?;
        tracing::info!("🔒 TLS enabled");
        Some(rc)
    } else {
        None
    };

    // Unified graceful shutdown via axum_server::Handle for both HTTP and HTTPS
    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_handle.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
    });

    // Build server future — the only difference is bind vs bind_rustls.
    // Boxing lets us treat both paths uniformly and eliminates the 4-way branch.
    let server_future: std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<()>> + Send>,
    > = if let Some(rustls_config) = rustls_config {
        Box::pin(
            axum_server::bind_rustls(sock_addr, rustls_config)
                .handle(handle.clone())
                .serve(app.into_make_service()),
        )
    } else {
        Box::pin(
            axum_server::bind(sock_addr)
                .handle(handle.clone())
                .serve(app.into_make_service()),
        )
    };

    if config.dashboard {
        let dashboard_metrics = Arc::clone(&metrics);
        let dashboard_log_buffer = Arc::clone(&log_buffer);
        let dashboard_shutdown = handle.clone();

        let dashboard_handle = tokio::spawn(async move {
            if let Err(e) = run_dashboard(dashboard_metrics, dashboard_log_buffer).await {
                eprintln!("Dashboard error: {}", e);
            }
            dashboard_shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
        });

        tokio::select! {
            result = server_future => {
                if let Err(e) = result {
                    tracing::error!("Server error: {}", e);
                }
            }
            _ = dashboard_handle => {
                tracing::info!("Dashboard closed, shutting down server...");
            }
        }
    } else {
        print_startup_banner(&config);
        tracing::info!("🚀 Server listening on {}://{}", protocol, sock_addr);

        server_future.await.context("Server error")?;
    }

    tracing::info!("👋 Server shutdown complete");

    Ok(())
}

/// Load models from Kiro API (no retries - fail fast during startup)
async fn load_models_from_kiro(
    http_client: &http_client::KiroHttpClient,
    auth_manager: &auth::AuthManager,
    _config: &config::Config,
) -> anyhow::Result<Vec<serde_json::Value>> {
    // Get access token
    let access_token = auth_manager.get_access_token().await?;
    let region = auth_manager.get_region().await;

    // Build request to list models - use Q API endpoint, not CodeWhisperer
    // Correct endpoint: https://q.{region}.amazonaws.com/ListAvailableModels
    let url = format!("https://q.{}.amazonaws.com/ListAvailableModels", region);

    // Build request with query parameters
    // Note: profileArn is only needed for Kiro Desktop auth, not AWS SSO OIDC
    let req_builder = http_client
        .client()
        .get(&url)
        .query(&[("origin", "AI_EDITOR")])
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json");

    // Add profileArn only if using Kiro Desktop auth (not AWS SSO)
    // For AWS SSO OIDC (kiro-cli), profileArn is not needed
    // TODO: Detect auth type and conditionally add profileArn

    let req = req_builder.build()?;

    // Execute request WITHOUT retries (fail fast during startup)
    let response = http_client.request_no_retry(req).await?;

    // Parse response
    let body = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&body)?;

    // Extract models from response
    if let Some(models) = json.get("models").and_then(|v| v.as_array()) {
        Ok(models.clone())
    } else {
        Ok(vec![])
    }
}

/// Add hidden models to cache
fn add_hidden_models(cache: &cache::ModelCache) {
    // Add commonly used model aliases that may not be in the API response
    let hidden_models = vec![
        (
            "claude-3-5-sonnet-20241022",
            "CLAUDE_3_5_SONNET_20241022_V2_0",
        ),
        (
            "claude-3-5-sonnet-20240620",
            "CLAUDE_3_5_SONNET_20240620_V1_0",
        ),
        (
            "claude-3-5-haiku-20241022",
            "CLAUDE_3_5_HAIKU_20241022_V1_0",
        ),
        ("claude-3-opus-20240229", "CLAUDE_3_OPUS_20240229_V1_0"),
        ("claude-3-sonnet-20240229", "CLAUDE_3_SONNET_20240229_V1_0"),
        ("claude-3-haiku-20240307", "CLAUDE_3_HAIKU_20240307_V1_0"),
        ("claude-sonnet-4", "CLAUDE_SONNET_4_20250514_V1_0"),
        ("claude-sonnet-4-20250514", "CLAUDE_SONNET_4_20250514_V1_0"),
        (
            "anthropic.claude-sonnet-4-v1",
            "CLAUDE_SONNET_4_20250514_V1_0",
        ),
    ];

    for (display_name, internal_id) in hidden_models {
        cache.add_hidden_model(display_name, internal_id);
    }
}

/// Build the application with all routes and middleware
fn build_app(state: routes::AppState) -> axum::Router {
    use axum::Router;

    // Health check routes (no auth required)
    let health_routes = routes::health_routes();

    // OpenAI API routes (with auth)
    let openai_routes = routes::openai_routes(state.clone());

    // Anthropic API routes (with auth)
    let anthropic_routes = routes::anthropic_routes(state.clone());

    // Combine all routes
    Router::new()
        .merge(health_routes)
        .merge(openai_routes)
        .merge(anthropic_routes)
        // Apply middleware stack: CORS → Debug → HSTS → (Auth is per-route)
        .layer(middleware::cors_layer())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::debug_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::hsts_middleware,
        ))
}

/// Print startup banner
fn print_startup_banner(config: &config::Config) {
    let banner = r#"
╔═══════════════════════════════════════════════════════════╗
║                                                           ║
║              🚀 Kiro Gateway - Rust Edition              ║
║                                                           ║
║  OpenAI & Anthropic compatible proxy for Kiro API        ║
║                                                           ║
╚═══════════════════════════════════════════════════════════╝
"#;

    println!("{}", banner);
    println!("  Version:     {}", env!("CARGO_PKG_VERSION"));
    let protocol = if config.is_tls_active() {
        "https"
    } else {
        "http"
    };
    println!(
        "  Server:      {}://{}:{}",
        protocol, config.server_host, config.server_port
    );
    if config.is_tls_active() {
        println!("  TLS:         enabled 🔒");
    }
    println!("  Region:      {}", config.kiro_region);
    println!("  Debug Mode:  {:?}", config.debug_mode);
    println!("  Log Level:   {}", config.log_level);
    println!(
        "  Fake Reasoning: {} (max_tokens: {})",
        if config.fake_reasoning_enabled {
            "enabled"
        } else {
            "disabled"
        },
        config.fake_reasoning_max_tokens
    );
    println!();
}

/// Handle graceful shutdown signal
async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C signal, initiating graceful shutdown...");
        },
        _ = terminate => {
            tracing::info!("Received terminate signal, initiating graceful shutdown...");
        },
    }
}

async fn run_dashboard(
    metrics: Arc<metrics::MetricsCollector>,
    log_buffer: Arc<Mutex<VecDeque<dashboard::app::LogEntry>>>,
) -> io::Result<()> {
    use crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::prelude::*;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = dashboard::DashboardApp::new(metrics, log_buffer.clone());
    let mut was_visible = true;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    loop {
        if app.dashboard_visible != was_visible {
            if app.dashboard_visible {
                enable_raw_mode()?;
                execute!(terminal.backend_mut(), EnterAlternateScreen)?;
                terminal.clear()?;
            } else {
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                disable_raw_mode()?;
                println!("\n--- Dashboard hidden. Press 'd' to show, 'q' to quit ---\n");

                if let Ok(logs) = log_buffer.lock() {
                    for entry in logs.iter().rev().take(20).rev() {
                        println!(
                            "[{}] {:5} {}",
                            entry.timestamp.format("%H:%M:%S"),
                            entry.level,
                            entry.message
                        );
                    }
                }
            }
            was_visible = app.dashboard_visible;
        }

        app.refresh_system_info();
        app.metrics.cleanup_old_samples();

        if app.dashboard_visible {
            terminal.draw(|frame| {
                dashboard::ui::render(frame, &app);
            })?;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        if !app.dashboard_visible {
            enable_raw_mode()?;
        }

        dashboard::event_handler::handle_events(&mut app)?;

        if !app.dashboard_visible {
            disable_raw_mode()?;
        }

        if app.should_quit {
            break;
        }
    }

    if app.dashboard_visible {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    terminal.show_cursor()?;

    Ok(())
}
