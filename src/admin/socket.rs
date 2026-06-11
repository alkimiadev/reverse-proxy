use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use serde_json;
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::ConfigReloadHandle;

#[derive(Debug, thiserror::Error)]
pub enum AdminSocketError {
    #[error("admin socket disabled (empty path)")]
    Disabled,
    #[error("socket file exists and is in use by another process: {0}")]
    SocketInUse(String),
    #[error("failed to bind admin socket: {0}")]
    BindFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Serialize)]
struct OkResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct OkWithUptimeResponse {
    status: &'static str,
    uptime_secs: u64,
    sites: usize,
}

#[derive(Serialize)]
struct ErrorResponse {
    status: &'static str,
    message: String,
}

pub struct AdminSocket {
    socket_path: String,
    reload_handle: Arc<ConfigReloadHandle>,
    config_path: String,
    start_time: Instant,
    reload_mutex: Arc<Mutex<()>>,
}

impl AdminSocket {
    pub fn new(
        socket_path: String,
        reload_handle: Arc<ConfigReloadHandle>,
        config_path: String,
    ) -> Self {
        let reload_mutex = Arc::new(Mutex::new(()));
        Self {
            socket_path,
            reload_handle,
            config_path,
            start_time: Instant::now(),
            reload_mutex,
        }
    }

    pub fn reload_mutex(&self) -> Arc<Mutex<()>> {
        self.reload_mutex.clone()
    }
}

pub async fn start_admin_socket(admin_socket: Arc<AdminSocket>) -> Result<(), AdminSocketError> {
    if admin_socket.socket_path.is_empty() {
        info!("admin socket disabled (empty path)");
        return Err(AdminSocketError::Disabled);
    }

    let socket_path = &admin_socket.socket_path;

    cleanup_stale_socket(socket_path).await?;

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            if e.kind() == io::ErrorKind::AddrInUse {
                warn!(
                    "admin socket path {} is in use by another process, disabling admin socket",
                    socket_path
                );
                return Err(AdminSocketError::SocketInUse(socket_path.clone()));
            }
            return Err(AdminSocketError::BindFailed(e.to_string()));
        }
    };

    info!("admin socket listening on {}", socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let admin_socket = admin_socket.clone();
                tokio::spawn(async move {
                    handle_connection(stream, admin_socket).await;
                });
            }
            Err(e) => {
                warn!("failed to accept admin socket connection: {}", e);
            }
        }
    }
}

async fn cleanup_stale_socket(path: &str) -> Result<(), AdminSocketError> {
    let socket_path = Path::new(path);
    if !socket_path.exists() {
        return Ok(());
    }

    if is_socket_active(path).await {
        warn!(
            "socket file {} exists and another process is listening; disabling admin socket",
            path
        );
        return Err(AdminSocketError::SocketInUse(path.to_string()));
    }

    warn!("removing stale socket file: {}", path);
    tokio::fs::remove_file(path)
        .await
        .map_err(AdminSocketError::Io)
}

async fn is_socket_active(path: &str) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
}

async fn handle_connection(stream: tokio::net::UnixStream, admin_socket: Arc<AdminSocket>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) | Err(_) => {
            let _ = writer
                .write_all(
                    format!(
                        "{}\n",
                        serde_json::to_string(&ErrorResponse {
                            status: "error",
                            message: "invalid input".to_string(),
                        })
                        .unwrap()
                    )
                    .as_bytes(),
                )
                .await;
            return;
        }
        _ => {}
    }

    let command = line.trim();
    let response = match command {
        "reload" => handle_reload(&admin_socket).await,
        "status" => handle_status(&admin_socket).await,
        "" => serde_json::to_string(&ErrorResponse {
            status: "error",
            message: "invalid input".to_string(),
        })
        .unwrap(),
        _ => serde_json::to_string(&ErrorResponse {
            status: "error",
            message: format!("unknown command: {}", command),
        })
        .unwrap(),
    };

    let _ = writer.write_all(format!("{}\n", response).as_bytes()).await;
}

async fn handle_reload(admin_socket: &Arc<AdminSocket>) -> String {
    let _guard = admin_socket.reload_mutex.lock().await;

    let config_content = match tokio::fs::read_to_string(&admin_socket.config_path).await {
        Ok(content) => content,
        Err(e) => {
            return serde_json::to_string(&ErrorResponse {
                status: "error",
                message: format!("failed to read config file: {}", e),
            })
            .unwrap();
        }
    };

    let full_config = match crate::config::FullConfig::parse(&config_content) {
        Ok(c) => c,
        Err(e) => {
            return serde_json::to_string(&ErrorResponse {
                status: "error",
                message: format!("failed to parse config file: {}", e),
            })
            .unwrap();
        }
    };

    let (new_static, new_dynamic) = full_config.into_static_and_dynamic();

    match admin_socket
        .reload_handle
        .reload(new_static, new_dynamic)
        .await
    {
        Ok(changed_fields) => {
            if !changed_fields.is_empty() {
                tracing::warn!(
                    "static config fields changed (restart required): {}",
                    changed_fields.join(", ")
                );
            }
            tracing::info!(event = "CONFIG_RELOAD", status = "success");
            serde_json::to_string(&OkResponse { status: "ok" }).unwrap()
        }
        Err(e) => {
            tracing::error!("config reload failed: {}", e);
            serde_json::to_string(&ErrorResponse {
                status: "error",
                message: e.to_string(),
            })
            .unwrap()
        }
    }
}

async fn handle_status(admin_socket: &Arc<AdminSocket>) -> String {
    let config = admin_socket.reload_handle.load();
    let uptime_secs = admin_socket.start_time.elapsed().as_secs();

    serde_json::to_string(&OkWithUptimeResponse {
        status: "ok",
        uptime_secs,
        sites: config.sites.len(),
    })
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_fixtures;
    use std::time::Duration;

    fn create_test_admin_socket(dir: &std::path::Path) -> AdminSocket {
        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config));
        AdminSocket::new(
            dir.join("admin.sock").to_string_lossy().to_string(),
            reload_handle,
            dir.join("config.toml").to_string_lossy().to_string(),
        )
    }

    #[tokio::test]
    async fn test_status_command() {
        let dir = tempfile::tempdir().unwrap();
        let admin_socket = Arc::new(create_test_admin_socket(dir.path()));
        let socket_path = dir.path().join("admin.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"status\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["uptime_secs"].is_number());
        assert!(parsed["sites"].is_number());
    }

    #[tokio::test]
    async fn test_unknown_command() {
        let dir = tempfile::tempdir().unwrap();
        let admin_socket = Arc::new(create_test_admin_socket(dir.path()));
        let socket_path = dir.path().join("admin.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"foo\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["message"], "unknown command: foo");
    }

    #[tokio::test]
    async fn test_empty_input() {
        let dir = tempfile::tempdir().unwrap();
        let admin_socket = Arc::new(create_test_admin_socket(dir.path()));
        let socket_path = dir.path().join("admin.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "error");
        assert_eq!(parsed["message"], "invalid input");
    }

    #[tokio::test]
    async fn test_reload_command_missing_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let admin_socket = Arc::new(create_test_admin_socket(dir.path()));
        let socket_path = dir.path().join("admin.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"reload\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("failed to read config file"));
    }

    #[tokio::test]
    async fn test_reload_command_success() {
        let dir = tempfile::tempdir().unwrap();

        let config_content = r#"
health_check_port = 9900
admin_socket_path = "/tmp/test-admin.sock"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "127.0.0.1"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_directory = "staging"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
        tokio::fs::write(dir.path().join("config.toml"), config_content)
            .await
            .unwrap();

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config));

        let admin_socket = Arc::new(AdminSocket::new(
            dir.path().join("admin.sock").to_string_lossy().to_string(),
            reload_handle,
            dir.path().join("config.toml").to_string_lossy().to_string(),
        ));

        let socket_path = dir.path().join("admin.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"reload\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "ok");
    }

    #[tokio::test]
    async fn test_cleanup_stale_socket_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        std::fs::write(&socket_path, "stale").unwrap();
        assert!(socket_path.exists());

        cleanup_stale_socket(socket_path.to_str().unwrap())
            .await
            .unwrap();
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_stale_socket_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("nonexistent.sock");

        cleanup_stale_socket(socket_path.to_str().unwrap())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_start_admin_socket_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config));

        let admin_socket = Arc::new(AdminSocket::new(
            String::new(),
            reload_handle,
            dir.path().join("config.toml").to_string_lossy().to_string(),
        ));

        let result = start_admin_socket(admin_socket).await;
        assert!(matches!(result, Err(AdminSocketError::Disabled)));
    }

    #[tokio::test]
    async fn test_start_admin_socket_detects_active_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("admin.sock");

        let _listener = UnixListener::bind(&socket_path).unwrap();

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config));

        let admin_socket = Arc::new(AdminSocket::new(
            socket_path.to_string_lossy().to_string(),
            reload_handle,
            dir.path().join("config.toml").to_string_lossy().to_string(),
        ));

        let result = start_admin_socket(admin_socket).await;
        assert!(matches!(result, Err(AdminSocketError::SocketInUse(_))));
    }

    #[tokio::test]
    async fn test_reload_serialized_with_mutex() {
        let dir = tempfile::tempdir().unwrap();

        let config_content = r#"
health_check_port = 9900
admin_socket_path = "/tmp/test-admin.sock"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "127.0.0.1"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_directory = "staging"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
        tokio::fs::write(dir.path().join("config.toml"), config_content)
            .await
            .unwrap();

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config));

        let admin_socket = Arc::new(AdminSocket::new(
            dir.path().join("admin.sock").to_string_lossy().to_string(),
            reload_handle,
            dir.path().join("config.toml").to_string_lossy().to_string(),
        ));

        let mutex = admin_socket.reload_mutex();
        let guard = mutex.lock().await;

        let socket_path = dir.path().join("admin.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"reload\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);

        tokio::select! {
            _ = reader.read_line(&mut response) => {},
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                drop(guard);
                reader.read_line(&mut response).await.unwrap();
            }
        }

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "ok");
    }

    #[tokio::test]
    async fn test_status_returns_sites_count() {
        let dir = tempfile::tempdir().unwrap();
        let admin_socket = Arc::new(create_test_admin_socket(dir.path()));
        let socket_path = dir.path().join("admin.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();

        let admin_socket_clone = admin_socket.clone();
        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, admin_socket_clone).await;
        });

        let mut stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        stream.write_all(b"status\n").await.unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        let mut reader = BufReader::new(stream);
        reader.read_line(&mut response).await.unwrap();

        handle.await.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["sites"], 1);
        assert!(parsed["uptime_secs"].is_number());
    }
}
