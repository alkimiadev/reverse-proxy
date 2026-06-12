pub mod format;

use crate::config::static_config::LoggingConfig;
use anyhow::Result;
use std::fs::File;
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

pub fn init(config: &LoggingConfig) -> Result<()> {
    let level = config.level.parse::<Level>().unwrap_or(Level::INFO);

    let env_filter = make_env_filter(level);

    match config.format.as_str() {
        "json" => init_json(env_filter, &config.log_file_path, level),
        _ => init_text(env_filter, &config.log_file_path, level),
    }
}

fn make_env_filter(level: Level) -> EnvFilter {
    EnvFilter::from_default_env().add_directive(level.into())
}

fn init_json(env_filter: EnvFilter, log_file_path: &Option<String>, level: Level) -> Result<()> {
    match log_file_path {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let file = File::create(path)?;
            let file_writer = Arc::new(file);

            let file_env_filter = make_env_filter(level);
            let stdout_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_ansi(false)
                .with_filter(env_filter);
            let file_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_ansi(false)
                .with_writer(file_writer)
                .with_filter(file_env_filter);
            tracing_subscriber::registry()
                .with(stdout_layer)
                .with(file_layer)
                .try_init()?;
        }
        None => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_filter(env_filter);
            tracing_subscriber::registry().with(layer).try_init()?;
        }
    }

    Ok(())
}

fn init_text(env_filter: EnvFilter, log_file_path: &Option<String>, level: Level) -> Result<()> {
    match log_file_path {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let file = File::create(path)?;
            let file_writer = Arc::new(file);

            let file_env_filter = make_env_filter(level);
            let stdout_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_filter(env_filter);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer)
                .with_filter(file_env_filter);
            tracing_subscriber::registry()
                .with(stdout_layer)
                .with(file_layer)
                .try_init()?;
        }
        None => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_filter(env_filter);
            tracing_subscriber::registry().with(layer).try_init()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::static_config::LoggingConfig;

    #[test]
    fn init_creates_log_directory_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("logs").join("access.log");
        let config = LoggingConfig {
            level: "info".to_string(),
            format: "text".to_string(),
            log_file_path: Some(log_path.to_string_lossy().to_string()),
        };

        if let Err(e) = init(&config) {
            let msg = format!("{e}");
            assert!(
                msg.contains("global default trace dispatcher") || msg.contains("already been set"),
                "unexpected init error: {e}"
            );
        }
        assert!(log_path.exists(), "log file should be created");
    }
}
