//! CLI binary for managing antigravity (Cloud Code) accounts.
//!
//! Subcommands: add, list, remove, verify

use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use kiro_gateway::antigravity::{account_storage, auth, auth_server, cloud_code_api, constants};

#[derive(Parser)]
#[command(name = "accounts", about = "Manage antigravity Cloud Code accounts")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new Google account via OAuth browser login
    Add,
    /// List all stored accounts
    List,
    /// Remove an account interactively
    Remove,
    /// Verify all stored accounts by refreshing their tokens
    Verify,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    match cli.command {
        Commands::Add => cmd_add().await,
        Commands::List => cmd_list(),
        Commands::Remove => cmd_remove(),
        Commands::Verify => cmd_verify().await,
    }
}

async fn cmd_add() -> Result<()> {
    let storage_path =
        account_storage::default_storage_path().context("Could not determine home directory")?;

    // Check OAuth credentials are configured
    if constants::OAUTH_CLIENT_ID.is_empty() || constants::OAUTH_CLIENT_SECRET.is_empty() {
        anyhow::bail!(
            "OAuth credentials not configured.\n\
             Set ANTIGRAVITY_OAUTH_CLIENT_ID and ANTIGRAVITY_OAUTH_CLIENT_SECRET in .env"
        );
    }

    // 1. Generate auth URL + PKCE + state
    let (auth_url, pkce, state) = auth::get_authorization_url(None);

    // 2. Start callback server before opening browser
    let server_future = auth_server::start_callback_server(&state, Duration::from_secs(120));

    // 3. Open browser (fallback: print URL)
    println!("Opening browser for Google sign-in...");
    if open::that(&auth_url).is_err() {
        println!("Could not open browser. Please visit this URL manually:");
        println!("{}", auth_url);
    }
    println!("Waiting for OAuth callback...");

    // 4. Await callback
    let callback = server_future.await.context("OAuth callback failed")?;
    let redirect_uri = format!("http://localhost:{}/oauth-callback", callback.port);

    // 5. Exchange code for tokens
    let http = reqwest::Client::new();
    let tokens = auth::exchange_code(&http, &callback.code, &pkce.verifier, Some(&redirect_uri))
        .await
        .map_err(|e| anyhow::anyhow!(e))
        .context("Token exchange failed")?;

    let refresh_token = tokens
        .refresh_token
        .context("No refresh token received (did you use prompt=consent?)")?;

    // 6. Get user email
    let email = auth::get_user_email(&http, &tokens.access_token)
        .await
        .context("Failed to get user email")?;
    println!("Authenticated as: {}", email);

    // 7. Discover project via loadCodeAssist
    print!("Discovering Cloud Code project... ");
    let project_id = cloud_code_api::load_code_assist(&http, &tokens.access_token).await?;

    let (project_id, managed_project_id) = match project_id {
        Some(pid) => {
            println!("found: {}", pid);
            (Some(pid.clone()), Some(pid))
        }
        None => {
            println!("none found, onboarding...");
            // 8. Onboard user to provision a project
            let managed = cloud_code_api::onboard_user(&http, &tokens.access_token, None).await?;
            println!("Provisioned project: {}", managed);
            (None, Some(managed))
        }
    };

    // 9. Build composite refresh token
    let parts = auth::RefreshParts {
        refresh_token,
        project_id,
        managed_project_id,
    };
    let composite = auth::format_refresh_parts(&parts);

    // 10. Load existing accounts, check for duplicates, save
    let mut accounts = account_storage::load_accounts(&storage_path)?;
    if accounts.iter().any(|a| a.email == email) {
        // Update existing account
        for a in &mut accounts {
            if a.email == email {
                a.composite_refresh_token = composite.clone();
                a.last_used = Some(chrono::Utc::now());
            }
        }
        println!("Updated existing account: {}", email);
    } else {
        accounts.push(account_storage::StoredAccount {
            email: email.clone(),
            composite_refresh_token: composite,
            added_at: chrono::Utc::now(),
            last_used: None,
        });
        println!("Added account: {}", email);
    }
    account_storage::save_accounts(&storage_path, &accounts)?;

    println!(
        "Saved to {}  ({} account{})",
        storage_path.display(),
        accounts.len(),
        if accounts.len() == 1 { "" } else { "s" }
    );

    Ok(())
}

fn cmd_list() -> Result<()> {
    let storage_path =
        account_storage::default_storage_path().context("Could not determine home directory")?;

    let accounts = account_storage::load_accounts(&storage_path)?;

    if accounts.is_empty() {
        println!("No accounts stored.");
        println!("Run `accounts add` to add one.");
        return Ok(());
    }

    println!("{:<4} {:<35} {:<22} Last Used", "#", "Email", "Added");
    println!("{}", "-".repeat(85));

    for (i, account) in accounts.iter().enumerate() {
        let added = account.added_at.format("%Y-%m-%d %H:%M UTC");
        let last_used = account
            .last_used
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".to_string());

        println!(
            "{:<4} {:<35} {:<22} {}",
            i + 1,
            account.email,
            added,
            last_used
        );
    }

    println!(
        "\n{} account{}",
        accounts.len(),
        if accounts.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

fn cmd_remove() -> Result<()> {
    let storage_path =
        account_storage::default_storage_path().context("Could not determine home directory")?;

    let mut accounts = account_storage::load_accounts(&storage_path)?;

    if accounts.is_empty() {
        println!("No accounts to remove.");
        return Ok(());
    }

    let items: Vec<String> = accounts.iter().map(|a| a.email.clone()).collect();

    let selection = dialoguer::Select::new()
        .with_prompt("Select account to remove")
        .items(&items)
        .default(0)
        .interact()?;

    let removed = accounts.remove(selection);
    account_storage::save_accounts(&storage_path, &accounts)?;

    println!("Removed: {}", removed.email);
    println!(
        "{} account{} remaining",
        accounts.len(),
        if accounts.len() == 1 { "" } else { "s" }
    );
    Ok(())
}

async fn cmd_verify() -> Result<()> {
    let storage_path =
        account_storage::default_storage_path().context("Could not determine home directory")?;

    // Load .env for OAuth credentials
    if constants::OAUTH_CLIENT_ID.is_empty() || constants::OAUTH_CLIENT_SECRET.is_empty() {
        anyhow::bail!(
            "OAuth credentials not configured.\n\
             Set ANTIGRAVITY_OAUTH_CLIENT_ID and ANTIGRAVITY_OAUTH_CLIENT_SECRET in .env"
        );
    }

    let accounts = account_storage::load_accounts(&storage_path)?;

    if accounts.is_empty() {
        println!("No accounts to verify.");
        return Ok(());
    }

    let http = reqwest::Client::new();
    let mut ok_count = 0u32;
    let mut fail_count = 0u32;

    for account in &accounts {
        print!("{:<35} ", account.email);
        match auth::refresh_access_token(&http, &account.composite_refresh_token).await {
            Ok(_) => {
                println!("OK");
                ok_count += 1;
            }
            Err(e) => {
                println!("FAIL: {}", e);
                fail_count += 1;
            }
        }
    }

    println!("\n{} OK, {} failed", ok_count, fail_count);
    Ok(())
}
