use anyhow::{Context, Result};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

mod auth;
mod cache;
mod config;
mod converters;
mod cost;
mod datadog;
mod error;
mod guardrails;
mod http_client;
mod middleware;
mod models;
mod providers;
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
    let is_proxy_only = config.is_proxy_only();

    // Set up logging
    let log_level = config.log_level.to_lowercase();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&log_level));

    {
        use tracing_subscriber::prelude::*;

        // When Datadog is enabled, use structured JSON logs so the Agent can parse
        // and correlate them with APM traces.  The `Layer for Option<L>` blanket
        // impl makes the inactive branch a zero-cost no-op.
        //
        // dd.trace_id and dd.span_id are recorded as span fields by a middleware
        // (not in the formatter, which would deadlock due to re-entrancy).
        let dd_configured = datadog::dd_agent_configured();
        let json_fmt = dd_configured.then(|| {
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_thread_ids(false)
                .with_current_span(true)
        });
        let text_fmt = (!dd_configured).then(|| {
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
        });

        // Datadog APM layer (innermost to avoid filtering events from fmt layer).
        let dd_layer = datadog::init_datadog();

        tracing_subscriber::registry()
            .with(dd_layer)
            .with(env_filter)
            .with(json_fmt)
            .with(text_fmt)
            .init();
    }

    if is_proxy_only {
        tracing::info!("Kiro Gateway starting in proxy-only mode...");
    } else {
        tracing::info!("Kiro Gateway starting...");
    }

    // ── Database ─────────────────────────────────────────────────
    let config_db = if is_proxy_only {
        None
    } else if let Some(ref url) = config.database_url {
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
    };

    // ── Setup state ─────────────────────────────────────────────────
    let mut setup_complete_flag = if is_proxy_only {
        true
    } else if let Some(ref db) = config_db {
        db.is_setup_complete().await
    } else {
        false
    };

    // Seed initial admin user from env vars (only on first run, before any users exist)
    if !is_proxy_only && !setup_complete_flag {
        if let Some(ref db) = config_db {
            let initial_email = std::env::var("INITIAL_ADMIN_EMAIL").ok();
            let initial_password = std::env::var("INITIAL_ADMIN_PASSWORD").ok();
            if let (Some(email), Some(password)) = (initial_email, initial_password) {
                match web_ui::password_auth::hash_password(&password).await {
                    Ok(password_hash) => {
                        match db
                            .create_password_user(&email, &email, &password_hash, "admin", false)
                            .await
                        {
                            Ok(user_id) => {
                                tracing::info!(
                                    user_id = %user_id,
                                    email = %email,
                                    "Initial admin user created from env vars"
                                );

                                // Pre-configure TOTP if secret is provided
                                if let Ok(totp_secret) = std::env::var("INITIAL_ADMIN_TOTP_SECRET")
                                {
                                    if !totp_secret.is_empty() {
                                        match db.enable_totp(user_id, &totp_secret).await {
                                            Ok(()) => {
                                                // Generate and store recovery codes
                                                use rand::Rng;
                                                use sha2::{Digest, Sha256};

                                                let mut rng = rand::thread_rng();
                                                let recovery_codes: Vec<String> = (0..8)
                                                    .map(|_| {
                                                        (0..8)
                                                            .map(|_| {
                                                                let idx = rng.gen_range(0..36u8);
                                                                if idx < 10 {
                                                                    (b'0' + idx) as char
                                                                } else {
                                                                    (b'a' + idx - 10) as char
                                                                }
                                                            })
                                                            .collect()
                                                    })
                                                    .collect();

                                                let code_hashes: Vec<String> = recovery_codes
                                                    .iter()
                                                    .map(|c| {
                                                        let mut hasher = Sha256::new();
                                                        hasher.update(c.as_bytes());
                                                        hex::encode(hasher.finalize())
                                                    })
                                                    .collect();

                                                match db
                                                    .store_recovery_codes(user_id, &code_hashes)
                                                    .await
                                                {
                                                    Ok(()) => {
                                                        tracing::info!(
                                                            user_id = %user_id,
                                                            "TOTP pre-configured for initial admin"
                                                        );
                                                        tracing::info!(
                                                            "Recovery codes (save these): {:?}",
                                                            recovery_codes
                                                        );
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            error = %e,
                                                            "Failed to store recovery codes for initial admin"
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    error = %e,
                                                    "Failed to enable TOTP for initial admin"
                                                );
                                            }
                                        }
                                    }
                                }

                                setup_complete_flag = true;
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "Failed to create initial admin user"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to hash initial admin password");
                    }
                }
            }
        }
    }

    let setup_complete = Arc::new(AtomicBool::new(setup_complete_flag));

    if setup_complete_flag && !is_proxy_only {
        if let Some(ref db) = config_db {
            db.load_into_config(&mut config)
                .await
                .context("Failed to load config from database")?;
            tracing::info!("Configuration loaded from database");
        }
    } else if is_proxy_only {
        tracing::info!("Proxy-only mode — skipping database config");
    } else {
        tracing::warn!("Setup not complete — starting in setup-only mode");
        tracing::warn!("Visit the web UI to complete initial setup");
    }

    tracing::info!(
        "Server configured: {}:{}",
        config.server_host,
        config.server_port
    );
    tracing::debug!("Debug mode: {:?}", config.debug_mode);

    // ── Auth manager ────────────────────────────────────────────────
    let app_auth_manager = if is_proxy_only {
        match auth::AuthManager::new_from_env(&config)
            .context("Failed to create auth manager from env vars")?
        {
            Some(am) => {
                tracing::info!("Bootstrapping Kiro proxy credentials...");
                am.bootstrap_proxy_credentials()
                    .await
                    .context("Failed to bootstrap Kiro credentials. Check KIRO_REFRESH_TOKEN.")?;
                am
            }
            None => {
                // No Kiro credentials — running with other providers only
                auth::AuthManager::new_placeholder(
                    config.kiro_region.clone(),
                    config.token_refresh_threshold,
                )
                .context("Failed to create placeholder auth manager")?
            }
        }
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
    let mut model_cache = cache::ModelCache::new(3600); // 1 hour TTL

    // Wire DB reference for registry-backed lookups
    if let Some(ref db) = config_db {
        model_cache.set_db(Arc::clone(db));
    }

    // Load models from Kiro API at startup (proxy-only or setup complete)
    if setup_complete_flag {
        if app_auth_manager.has_credentials().await {
            tracing::info!("Loading models from Kiro API...");
            match load_models_from_kiro(&http_client, &app_auth_manager, &config).await {
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
            tracing::info!(
                "No shared credentials — model list will be populated on first user request"
            );
        }
    } else {
        tracing::info!("Skipping model loading — setup not complete");
    }

    // Add hidden models to cache
    add_hidden_models(&model_cache);

    // ── Model registry cache ───────────────────────────────────
    // Load admin-enabled registry models into in-memory cache.
    // Models are populated on-demand via the admin UI, not on startup.
    if let Some(ref _db) = config_db {
        match model_cache.load_from_registry().await {
            Ok(count) => {
                tracing::info!(count, "Loaded registry models into cache");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load registry models into cache");
            }
        }
    }

    let resolver =
        resolver::ModelResolver::new(model_cache.clone(), std::collections::HashMap::new());
    tracing::info!("Model resolver initialized");

    // Initialise Datadog OTLP metrics pipeline (no-op when DD_AGENT_HOST is unset)
    let otel_metrics_provider = datadog::init_otel_metrics();

    // Compute proxy API key hash at startup for constant-time comparison in middleware
    let proxy_api_key_hash = config.proxy.as_ref().map(|p| {
        use sha2::{Digest, Sha256};
        let hash: [u8; 32] = Sha256::digest(p.api_key.as_bytes()).into();
        hash
    });

    let auth_manager = Arc::new(tokio::sync::RwLock::new(app_auth_manager));
    let config_arc = Arc::new(RwLock::new(config.clone()));

    // Build provider registry (with proxy credentials in proxy mode)
    let provider_registry = if config.is_proxy_only() {
        let registry = if let Some(ref proxy) = config.proxy {
            providers::registry::ProviderRegistry::from_proxy_config(proxy)
        } else {
            providers::registry::ProviderRegistry::new()
        };
        if let Some(ref creds) = registry.proxy_credentials() {
            let count = creds.len();
            tracing::info!(count, "Proxy mode: loaded provider credentials from env");
        }
        Arc::new(registry)
    } else {
        Arc::new(providers::registry::ProviderRegistry::new())
    };

    let mut app_state = routes::AppState {
        proxy_api_key_hash,
        model_cache: model_cache.clone(),
        auth_manager: Arc::clone(&auth_manager),
        http_client: http_client.clone(),
        resolver,
        config: Arc::clone(&config_arc),
        setup_complete: Arc::clone(&setup_complete),
        config_db,
        session_cache: Arc::new(dashmap::DashMap::new()),
        api_key_cache: Arc::new(dashmap::DashMap::new()),
        kiro_token_cache: Arc::new(dashmap::DashMap::new()),
        oauth_pending: Arc::new(dashmap::DashMap::new()),
        guardrails_engine: None,
        provider_registry,
        providers: providers::build_provider_map(
            http_client.clone(),
            Arc::clone(&auth_manager),
            Arc::clone(&config_arc),
        ),
        provider_oauth_pending: Arc::new(dashmap::DashMap::new()),
        token_exchanger: Arc::new(web_ui::provider_oauth::HttpTokenExchanger::with_config(
            Arc::clone(&config_arc),
        )),
        login_rate_limiter: Arc::new(dashmap::DashMap::new()),
        rate_tracker: Arc::new(providers::rate_limiter::RateLimitTracker::new()),
    };

    // ── Guardrails ────────────────────────────────────────────────
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

    // ── Background tasks ─────────────────────────────────────────
    if let Some(ref db) = app_state.config_db {
        web_ui::user_kiro::spawn_token_refresh_task(Arc::clone(db));
        // Get copilot token cache from the CopilotProvider for the refresh task
        if let Some(copilot) = app_state
            .providers
            .get(&providers::types::ProviderId::Copilot)
            .and_then(|p| {
                p.as_any()
                    .downcast_ref::<providers::copilot::CopilotProvider>()
            })
        {
            web_ui::copilot_auth::spawn_copilot_token_refresh_task(
                Arc::clone(db),
                Arc::clone(copilot.token_cache()),
            );
        }
        web_ui::session::SessionService::spawn_cleanup_task(
            Arc::clone(db),
            Arc::clone(&app_state.session_cache),
        );

        // Model registry: auto-populate on startup + periodic refresh
        if setup_complete_flag {
            spawn_model_registry_tasks(
                Arc::clone(db),
                http_client.clone(),
                Arc::clone(&auth_manager),
                model_cache.clone(),
            );
        }

        tracing::info!("Background tasks started (token refresh, session cleanup)");
    }

    let app = build_app(app_state, is_proxy_only);

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

    // Flush and shut down Datadog APM tracer and OTLP metrics (no-op when DD_AGENT_HOST is unset)
    datadog::shutdown(otel_metrics_provider.as_ref());

    tracing::info!("Server shutdown complete");

    Ok(())
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

/// Spawn background tasks for model registry: initial populate + periodic refresh.
fn spawn_model_registry_tasks(
    db: Arc<web_ui::config_db::ConfigDb>,
    http_client: Arc<http_client::KiroHttpClient>,
    auth_manager: Arc<tokio::sync::RwLock<auth::AuthManager>>,
    model_cache: cache::ModelCache,
) {
    tokio::spawn(async move {
        // Initial populate on startup
        populate_all_providers(&db, &http_client, &auth_manager, &model_cache).await;

        // Periodic refresh (every 6 hours)
        let refresh_interval = std::time::Duration::from_secs(6 * 60 * 60);
        loop {
            tokio::time::sleep(refresh_interval).await;
            tracing::info!("Running periodic model registry refresh");
            populate_all_providers(&db, &http_client, &auth_manager, &model_cache).await;
        }
    });
}

/// Populate all provider model registries, then reload the cache.
async fn populate_all_providers(
    db: &Arc<web_ui::config_db::ConfigDb>,
    http_client: &Arc<http_client::KiroHttpClient>,
    auth_manager: &Arc<tokio::sync::RwLock<auth::AuthManager>>,
    model_cache: &cache::ModelCache,
) {
    for pid in providers::types::ProviderId::all_visible() {
        let provider_id = pid.as_str();
        let auth = if provider_id == "kiro" {
            let guard = auth_manager.read().await;
            if guard.has_credentials().await {
                // Cannot hold the guard across populate_provider, so fetch kiro directly
                match web_ui::model_registry::fetch_kiro_models(http_client, &guard).await {
                    Ok(models) if !models.is_empty() => {
                        drop(guard);
                        match db.bulk_upsert_registry_models(&models).await {
                            Ok(count) => {
                                tracing::info!(provider = "kiro", count, "Populated models");
                            }
                            Err(e) => {
                                tracing::warn!(provider = "kiro", error = %e, "Failed to upsert");
                            }
                        }
                    }
                    _ => {
                        drop(guard);
                    }
                }
                continue;
            } else {
                drop(guard);
                continue;
            }
        } else {
            None
        };

        match web_ui::model_registry::populate_provider(provider_id, db, http_client, auth).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(provider = provider_id, count, "Populated models");
                }
            }
            Err(e) => {
                tracing::warn!(provider = provider_id, error = %e, "Failed to populate");
            }
        }
    }

    // Reload registry cache after populating all providers
    if let Err(e) = model_cache.load_from_registry().await {
        tracing::warn!(error = %e, "Failed to reload registry cache after populate");
    }
}

/// Build the application with all routes and middleware
fn build_app(state: routes::AppState, is_proxy_only: bool) -> axum::Router {
    use axum::Router;

    let health_routes = routes::health_routes();

    let openai_routes = routes::openai_routes(state.clone()).layer(
        axum::middleware::from_fn_with_state(state.clone(), crate::web_ui::setup_guard),
    );

    let anthropic_routes = routes::anthropic_routes(state.clone()).layer(
        axum::middleware::from_fn_with_state(state.clone(), crate::web_ui::setup_guard),
    );

    let mut app = Router::new()
        .merge(health_routes)
        .merge(openai_routes)
        .merge(anthropic_routes);

    if !is_proxy_only {
        let web_ui = web_ui::web_ui_routes(state.clone());
        app = app.merge(web_ui);
    }

    app.layer(middleware::cors_layer())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::debug_middleware,
        ))
        // Record dd.trace_id/dd.span_id on the http_request span for Datadog
        // log-trace correlation.  Runs inside the TraceLayer span.
        .layer(axum::middleware::from_fn(datadog::dd_context_middleware))
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                |request: &axum::http::Request<axum::body::Body>| {
                    let path = request.uri().path();
                    // Generate a short request ID for correlation across log lines.
                    let request_id = &uuid::Uuid::new_v4().to_string()[..8];
                    if path == "/health" || path == "/" {
                        tracing::debug_span!(
                            "http_request",
                            method = %request.method(),
                            path = %path,
                            request_id = %request_id,
                            usr.id = tracing::field::Empty,
                            dd.trace_id = tracing::field::Empty,
                            dd.span_id = tracing::field::Empty,
                        )
                    } else {
                        tracing::info_span!(
                            "http_request",
                            method = %request.method(),
                            path = %path,
                            request_id = %request_id,
                            usr.id = tracing::field::Empty,
                            dd.trace_id = tracing::field::Empty,
                            dd.span_id = tracing::field::Empty,
                        )
                    }
                },
            ),
        )
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
    if config.is_proxy_only() {
        println!("  Mode:        proxy-only (no DB, no Web UI)");
    }
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
