use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

mod auth;
mod cache;
mod config;
mod converters;
mod error;
mod guardrails;
mod http_client;
mod mcp;
mod log_capture;
mod metrics;
mod middleware;
mod models;
mod resolver;
mod routes;
mod streaming;
mod thinking_parser;
mod tokenizer;
mod truncation;
mod utils;
mod web_ui;

#[tokio::main]
async fn main() -> Result<()> {
    // Load bootstrap configuration from environment variables
    let mut config = config::Config::load()?;
    config.validate()?;

    let log_buffer = Arc::new(Mutex::new(VecDeque::new()));

    // Set up logging with capture layer for web UI SSE log streaming
    let log_level = config.log_level.to_lowercase();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_level));

    {
        use tracing_subscriber::prelude::*;
        let capture_layer = log_capture::LogCaptureLayer::new(Arc::clone(&log_buffer));
        tracing_subscriber::registry()
            .with(env_filter)
            .with(capture_layer)
            .init();
    }

    let is_proxy_only = config.is_proxy_only();

    if is_proxy_only {
        tracing::info!("Kiro Gateway starting in PROXY-ONLY mode...");
    } else {
        tracing::info!("Kiro Gateway starting...");
    }

    // ── Database (skip in proxy-only mode) ──────────────────────────
    let config_db = if !is_proxy_only {
        if let Some(ref url) = config.database_url {
            match web_ui::config_db::ConfigDb::connect(url).await {
                Ok(db) => {
                    tracing::info!("Connected to PostgreSQL database");
                    Some(Arc::new(db))
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to database: {}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // ── Setup state ─────────────────────────────────────────────────
    let setup_complete_flag = if is_proxy_only {
        true // proxy-only is always "setup complete"
    } else if let Some(ref db) = config_db {
        db.is_setup_complete().await
    } else {
        false
    };

    let setup_complete = Arc::new(AtomicBool::new(setup_complete_flag));

    if !is_proxy_only {
        if setup_complete_flag {
            if let Some(ref db) = config_db {
                db.load_into_config(&mut config)
                    .await
                    .context("Failed to load config from database")?;
                tracing::info!("Configuration loaded from database");
            }
        } else {
            tracing::warn!("Setup not complete — starting in setup-only mode");
            tracing::warn!("Visit the web UI to complete initial setup");
        }
    }

    tracing::info!(
        "Server configured: {}:{}",
                    config.server_host, config.server_port
    );
    tracing::debug!("Debug mode: {:?}", config.debug_mode);

    // ── Auth manager ────────────────────────────────────────────────
    let app_auth_manager = if is_proxy_only {
        let am = auth::AuthManager::new_from_env(&config)
            .context("Failed to create auth manager from env vars")?;
        tracing::info!("Bootstrapping proxy-only credentials...");
        am.bootstrap_proxy_credentials()
            .await
            .context("Failed to bootstrap proxy credentials. Check KIRO_REFRESH_TOKEN and KIRO_SSO_REGION.")?;
        am
    } else if setup_complete_flag {
        init_app_auth_from_config_db(&config, &config_db).await?
    } else {
        auth::AuthManager::new_placeholder(
            config.kiro_region.clone(),
            config.token_refresh_threshold,
        )
        .context("Failed to create placeholder auth manager for AppState")?
    };

    // ── HTTP client ─────────────────────────────────────────────────
    let http_client = Arc::new(http_client::KiroHttpClient::new(
        config.http_max_connections,
        config.http_connect_timeout,
        config.http_request_timeout,
        config.http_max_retries,
    )?);
    tracing::info!("HTTP client initialized with connection pooling");

    // ── Model cache ─────────────────────────────────────────────────
    tracing::info!("Initializing model cache...");
    let model_cache = cache::ModelCache::new(3600); // 1 hour TTL

    // Load models from Kiro API at startup (proxy-only or setup complete)
    if setup_complete_flag {
        tracing::info!("Loading models from Kiro API...");
        // For proxy-only, wrap the env-bootstrapped auth manager temporarily
        let auth_for_models = if is_proxy_only {
            Arc::new(auth::AuthManager::new_from_env(&config)?)
        } else {
            init_auth_from_config_db(&config, &config_db).await?
        };
        match load_models_from_kiro(&http_client, &auth_for_models, &config).await {
            Ok(models) => {
                tracing::info!("Models from Kiro API:");
                for model in &models {
                    tracing::info!(
                        "{}",
                        serde_json::to_string_pretty(model).unwrap_or_default()
                    );
                }
                model_cache.update(models);
                tracing::info!(
                    "Loaded {} models from Kiro API",
                    model_cache.get_all_model_ids().len()
                );
            }
            Err(e) => {
                tracing::error!("Failed to load models from Kiro API: {}", e);
                tracing::warn!("Server will start but model list will be empty");
            }
        }
    } else {
        tracing::info!("Skipping model loading — setup not complete");
    }

    // Add hidden models to cache
    add_hidden_models(&model_cache);

    let resolver =
        resolver::ModelResolver::new(model_cache.clone(), std::collections::HashMap::new());
    tracing::info!("Model resolver initialized");

    let metrics = Arc::new(metrics::MetricsCollector::new());
    tracing::info!("Metrics collector initialized");

    let mut app_state = routes::AppState {
        model_cache: model_cache.clone(),
        auth_manager: Arc::new(tokio::sync::RwLock::new(app_auth_manager)),
        http_client: http_client.clone(),
        resolver,
        config: Arc::new(RwLock::new(config.clone())),
        setup_complete: Arc::clone(&setup_complete),
        metrics: Arc::clone(&metrics),
        log_buffer: Arc::clone(&log_buffer),
        config_db,
        session_cache: Arc::new(dashmap::DashMap::new()),
        api_key_cache: Arc::new(dashmap::DashMap::new()),
        kiro_token_cache: Arc::new(dashmap::DashMap::new()),
        oauth_pending: Arc::new(dashmap::DashMap::new()),
        guardrails_engine: None,
        mcp_manager: None,
    };

    // ── Guardrails (skip in proxy-only mode) ────────────────────────
    if !is_proxy_only {
        if let Some(ref db) = app_state.config_db {
            let guardrails_db = guardrails::db::GuardrailsDb::new(db.pool().clone());
            match guardrails::engine::GuardrailsEngine::new(&guardrails_db, config.guardrails_enabled)
                .await
            {
                Ok(engine) => {
                    app_state.guardrails_engine = Some(Arc::new(engine));
                    tracing::info!(
                        "Guardrails engine initialized (enabled: {})",
                        config.guardrails_enabled
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to initialize guardrails engine: {}", e);
                }
            }
        }
    }

    // ── MCP Gateway (skip in proxy-only mode) ───────────────────────
    if !is_proxy_only && config.mcp_enabled {
        if let Some(ref db) = app_state.config_db {
            let mcp_db = mcp::db::McpDb::new(db.pool().clone());
            let mgr = mcp::McpManager::new(
                Arc::new(mcp_db),
                config.mcp_tool_execution_timeout,
                config.mcp_health_check_interval,
                config.mcp_max_consecutive_failures,
            );
            mgr.initialize().await;
            app_state.mcp_manager = Some(Arc::new(mgr));
            tracing::info!("MCP Gateway initialized");
        }
    }

    // ── Background tasks (skip in proxy-only mode) ──────────────────
    if !is_proxy_only {
        if let Some(ref db) = app_state.config_db {
            web_ui::user_kiro::spawn_token_refresh_task(Arc::clone(db));
            web_ui::session::SessionService::spawn_cleanup_task(
                Arc::clone(db),
                Arc::clone(&app_state.session_cache),
            );
            tracing::info!("Background tasks started (token refresh, session cleanup)");
        }
    }

    // Clone mcp_manager reference before moving app_state into the router
    let mcp_manager_ref = app_state.mcp_manager.clone();

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

    print_startup_banner(&config);
    tracing::info!("Server listening on http://{}", sock_addr);

    let listener = tokio::net::TcpListener::bind(sock_addr)
        .await
        .context("Failed to bind server")?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    // Gracefully shutdown MCP Gateway (abort background tasks, close transports)
    if let Some(ref mcp) = mcp_manager_ref {
        mcp.shutdown().await;
    }

    tracing::info!("Server shutdown complete");

    Ok(())
}

/// Initialize AuthManager from config DB (Arc-wrapped, for the http_client).
async fn init_auth_from_config_db(
    config: &config::Config,
    config_db: &Option<Arc<web_ui::config_db::ConfigDb>>,
) -> Result<Arc<auth::AuthManager>> {
    if let Some(ref db) = config_db {
        tracing::info!("Initializing authentication from config database...");
        match auth::AuthManager::new(Arc::clone(db), config.token_refresh_threshold).await {
            Ok(am) => {
                let am = Arc::new(am);
                match am.get_access_token().await {
                    Ok(token) => {
                        tracing::info!("Authentication successful (token length: {})", token.len());
                    }
                    Err(e) => {
                        tracing::error!("Authentication failed: {}", e);
                        tracing::warn!(
                            "Server will start but API requests will fail without valid credentials"
                        );
                    }
                }
                Ok(am)
            }
            Err(e) => {
                tracing::error!("Failed to initialize auth manager: {}", e);
                tracing::warn!("Starting with dummy auth — API requests will fail");
                Ok(Arc::new(
                    auth::AuthManager::new_placeholder(
                        config.kiro_region.clone(),
                        config.token_refresh_threshold,
                    )
                    .context("Failed to create fallback auth manager")?,
                ))
            }
        }
    } else {
        Ok(Arc::new(
            auth::AuthManager::new_placeholder(
                config.kiro_region.clone(),
                config.token_refresh_threshold,
            )
            .context("Failed to create fallback auth manager")?,
        ))
    }
}

/// Initialize AuthManager from config DB (unwrapped, for AppState).
async fn init_app_auth_from_config_db(
    config: &config::Config,
    config_db: &Option<Arc<web_ui::config_db::ConfigDb>>,
) -> Result<auth::AuthManager> {
    if let Some(ref db) = config_db {
        match auth::AuthManager::new(Arc::clone(db), config.token_refresh_threshold).await {
            Ok(am) => Ok(am),
            Err(_) => auth::AuthManager::new_placeholder(
                config.kiro_region.clone(),
                config.token_refresh_threshold,
            )
            .context("Failed to create fallback auth manager for AppState"),
        }
    } else {
        auth::AuthManager::new_placeholder(
            config.kiro_region.clone(),
            config.token_refresh_threshold,
        )
        .context("Failed to create fallback auth manager for AppState")
    }
}

/// Load models from Kiro API (no retries - fail fast during startup)
async fn load_models_from_kiro(
    http_client: &http_client::KiroHttpClient,
    auth_manager: &auth::AuthManager,
    _config: &config::Config,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let access_token = auth_manager.get_access_token().await?;
    let region = auth_manager.get_region().await;

    let url = format!("https://q.{}.amazonaws.com/ListAvailableModels", region);

    let req_builder = http_client
        .client()
        .get(&url)
        .query(&[("origin", "AI_EDITOR")])
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json");

    let req = req_builder.build()?;
    let response = http_client.request_no_retry(req).await?;
    let body = response.text().await?;
    let json: serde_json::Value = serde_json::from_str(&body)?;

    if let Some(models) = json.get("models").and_then(|v| v.as_array()) {
        Ok(models.clone())
    } else {
        Ok(vec![])
    }
}

/// Add hidden models to cache
fn add_hidden_models(cache: &cache::ModelCache) {
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

    let health_routes = routes::health_routes();

    let openai_routes = routes::openai_routes(state.clone()).layer(
        axum::middleware::from_fn_with_state(state.clone(), crate::web_ui::setup_guard),
    );

    let anthropic_routes = routes::anthropic_routes(state.clone()).layer(
        axum::middleware::from_fn_with_state(state.clone(), crate::web_ui::setup_guard),
    );

    let web_ui = web_ui::web_ui_routes(state.clone());

    let mut router = Router::new()
        .merge(health_routes)
        .merge(openai_routes)
        .merge(anthropic_routes)
        .merge(web_ui);

    // Register MCP routes if manager is available
    if state.mcp_manager.is_some() {
        use axum::routing::post;

        // /v1/mcp/tool/execute — authenticated tool execution
        let mcp_tool_routes = Router::new()
            .route("/v1/mcp/tool/execute", post(mcp::api::execute_tool_handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                middleware::auth_middleware,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::web_ui::setup_guard,
            ))
            .with_state(state.clone());

        // /mcp — MCP server endpoint (JSON-RPC POST + SSE GET)
        let mcp_server_routes = Router::new()
            .route(
                "/mcp",
                post(mcp::server::mcp_post_handler).get(mcp::server::mcp_sse_handler),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                middleware::auth_middleware,
            ))
            .with_state(state.clone());

        router = router.merge(mcp_tool_routes).merge(mcp_server_routes);
    }

    router
        .layer(middleware::cors_layer())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::debug_middleware,
        ))
}

/// Print startup banner
fn print_startup_banner(config: &config::Config) {
    let banner = r#"
╔═══════════════════════════════════════════════════════════╗
║                                                           ║
║              Kiro Gateway - Rust Edition                  ║
║                                                           ║
║  OpenAI & Anthropic compatible proxy for Kiro API        ║
║                                                           ║
╚═══════════════════════════════════════════════════════════╝
"#;

    println!("{}", banner);
    println!("  Version:     {}", env!("CARGO_PKG_VERSION"));
    println!(
        "  Server:      http://{}:{}",
        config.server_host, config.server_port
    );
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
    println!(
        "  Web UI:      http://{}:{}/_ui/",
        config.server_host, config.server_port
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
