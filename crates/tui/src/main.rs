use anyhow::Result;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod ui;
mod components;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    let debug = std::env::args().any(|a| a == "--debug");

    // Auto-load ~/.config/orchestrateur-llm/.env so keys work without shell sourcing.
    load_env_file();

    let filter = EnvFilter::from_default_env()
        .add_directive(if debug { "debug".parse()? } else { "info".parse()? });

    if debug {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();

        tracing::info!("=== orchestrateur-llm DEBUG MODE ===");
        return run_debug_check().await;
    }

    // Normal mode: log to file only — stderr would corrupt the TUI alternate screen.
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("orchestrateur-llm");

    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::never(&log_dir, "orchestrateur-llm.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    tracing::info!("log: {}", log_dir.join("orchestrateur-llm.log").display());

    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default().trim().to_string();
    let gemini_key    = std::env::var("GEMINI_API_KEY").unwrap_or_default().trim().to_string();
    let grok_key      = std::env::var("GROK_API_KEY").unwrap_or_default().trim().to_string();
    let gpt_key       = std::env::var("OPENAI_API_KEY").unwrap_or_default().trim().to_string();
    let project_path  = std::env::current_dir()?;

    tracing::info!(
        "keys — anthropic:{} gemini:{} grok:{} gpt:{}",
        if anthropic_key.is_empty() || anthropic_key.contains("REPLACE_ME") { "absent" } else { "ok" },
        if gemini_key.is_empty()    || gemini_key.contains("REPLACE_ME")    { "absent" } else { "ok" },
        if grok_key.is_empty()      || grok_key.contains("REPLACE_ME")      { "absent" } else { "ok" },
        if gpt_key.is_empty()       || gpt_key.contains("REPLACE_ME")       { "absent" } else { "ok" },
    );

    let mut app = App::new(project_path, anthropic_key, gemini_key, grok_key, gpt_key).await?;
    app.run().await
}

/// Parses ~/.config/orchestrateur-llm/.env and sets missing env vars.
/// Vars already set in the environment take precedence.
fn load_env_file() {
    let env_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config/orchestrateur-llm/.env");

    let content = match std::fs::read_to_string(&env_path) {
        Ok(c)  => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() { continue; }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim_matches('\'');
            if !key.is_empty() && !val.is_empty() && std::env::var(key).is_err() {
                std::env::set_var(key, val);
            }
        }
    }
}

async fn run_debug_check() -> Result<()> {
    let project_path = std::env::current_dir()?;
    tracing::info!("project path  : {}", project_path.display());

    let env_path = dirs::home_dir().unwrap_or_default().join(".config/orchestrateur-llm/.env");
    tracing::info!(".env path     : {} ({})",
        env_path.display(),
        if env_path.exists() { "found" } else { "missing" });

    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default().trim().to_string();
    let gemini_key    = std::env::var("GEMINI_API_KEY").unwrap_or_default().trim().to_string();
    let grok_key      = std::env::var("GROK_API_KEY").unwrap_or_default().trim().to_string();

    if anthropic_key.is_empty() || anthropic_key.contains("REPLACE_ME") {
        tracing::warn!("ANTHROPIC_API_KEY : absent");
    } else {
        tracing::info!("ANTHROPIC_API_KEY : {}… (ok)", &anthropic_key[..8.min(anthropic_key.len())]);
    }

    if gemini_key.is_empty() || gemini_key.contains("REPLACE_ME") {
        tracing::warn!("GEMINI_API_KEY    : absent");
    } else {
        tracing::info!("GEMINI_API_KEY    : {}… (ok)", &gemini_key[..8.min(gemini_key.len())]);
    }

    if grok_key.is_empty() || grok_key.contains("REPLACE_ME") {
        tracing::warn!("GROK_API_KEY      : absent");
    } else {
        tracing::info!("GROK_API_KEY      : {}… (ok)", &grok_key[..8.min(grok_key.len())]);
    }

    let config_path = dirs::home_dir().unwrap_or_default().join(".config/orchestrateur-llm/config.toml");
    tracing::info!("config.toml   : {} ({})",
        config_path.display(),
        if config_path.exists() { "ok" } else { "missing — run install.sh" });

    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("orchestrateur-llm");
    tracing::info!("log file      : {}", log_dir.join("orchestrateur-llm.log").display());

    tracing::info!("local LLM     : {}", if local_llm::is_model_loaded() { "loaded" } else { "not loaded" });

    let gpt_key = std::env::var("OPENAI_API_KEY").unwrap_or_default().trim().to_string();
    if gpt_key.is_empty() || gpt_key.contains("REPLACE_ME") {
        tracing::warn!("OPENAI_API_KEY    : absent");
    } else {
        tracing::info!("OPENAI_API_KEY    : {}… (ok)", &gpt_key[..8.min(gpt_key.len())]);
    }

    tracing::info!("initialising App…");
    match App::new(project_path, anthropic_key, gemini_key, grok_key, gpt_key).await {
        Ok(_)  => tracing::info!("App init      : OK"),
        Err(e) => tracing::error!("App init      : FAILED — {:#}", e),
    }

    tracing::info!("=== debug check complete ===");
    Ok(())
}
