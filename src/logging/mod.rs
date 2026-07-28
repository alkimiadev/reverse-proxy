pub mod format;
pub mod reopen;

use crate::config::static_config::LoggingConfig;
use anyhow::Result;
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

pub struct LogInit {
    pub reopen_handle: Option<reopen::LogReopenHandle>,
}

pub fn init(config: &LoggingConfig) -> Result<LogInit> {
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

fn init_json(
    env_filter: EnvFilter,
    log_file_path: &Option<String>,
    level: Level,
) -> Result<LogInit> {
    match log_file_path {
        Some(path) => {
            let path_std = std::path::Path::new(path);
            let file_writer = reopen::ReopenableFileWriter::new(path_std)?;
            let reopen_handle = file_writer.handle_with_path(path_std.to_path_buf());

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

            Ok(LogInit {
                reopen_handle: Some(reopen_handle),
            })
        }
        None => {
            let layer = tracing_subscriber::fmt::layer()
                .json()
                .with_ansi(false)
                .with_filter(env_filter);
            tracing_subscriber::registry()
                .with(layer)
                .try_init()?;

            Ok(LogInit {
                reopen_handle: None,
            })
        }
    }
}

fn init_text(
    env_filter: EnvFilter,
    log_file_path: &Option<String>,
    level: Level,
) -> Result<LogInit> {
    match log_file_path {
        Some(path) => {
            let path_std = std::path::Path::new(path);
            let file_writer = reopen::ReopenableFileWriter::new(path_std)?;
            let reopen_handle = file_writer.handle_with_path(path_std.to_path_buf());

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

            Ok(LogInit {
                reopen_handle: Some(reopen_handle),
            })
        }
        None => {
            let layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_filter(env_filter);
            tracing_subscriber::registry()
                .with(layer)
                .try_init()?;

            Ok(LogInit {
                reopen_handle: None,
            })
        }
    }
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

    #[test]
    fn init_returns_reopen_handle_when_file_configured() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("access.log");
        let config = LoggingConfig {
            level: "info".to_string(),
            format: "text".to_string(),
            log_file_path: Some(log_path.to_string_lossy().to_string()),
        };

        let result = init(&config);
        match result {
            Ok(init_result) => {
                assert!(init_result.reopen_handle.is_some());
            }
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("global default trace dispatcher")
                        || msg.contains("already been set"),
                    "unexpected init error: {e}"
                );
            }
        }
    }

    #[test]
    fn init_returns_no_reopen_handle_when_no_file() {
        let config = LoggingConfig {
            level: "info".to_string(),
            format: "text".to_string(),
            log_file_path: None,
        };

        let result = init(&config);
        match result {
            Ok(init_result) => {
                assert!(init_result.reopen_handle.is_none());
            }
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("global default trace dispatcher")
                        || msg.contains("already been set"),
                    "unexpected init error: {e}"
                );
            }
        }
    }
}
