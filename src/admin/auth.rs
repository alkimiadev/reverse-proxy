use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use arc_swap::ArcSwap;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum AdminKeyError {
    #[error("admin key file is not readable: {0}")]
    NotReadable(String),
    #[error("admin key file is empty")]
    EmptyFile,
}

pub struct AdminAuthConfig {
    pub admin_key_hash: [u8; 32],
}

pub fn load_admin_key(path: &str) -> Result<Option<[u8; 32]>, AdminKeyError> {
    if path.is_empty() {
        return Ok(None);
    }

    let key_content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            warn!("admin key file '{}' not readable, disabling admin endpoints: {}", path, e);
            return Ok(None);
        }
    };

    let key_trimmed = key_content.trim();
    if key_trimmed.is_empty() {
        return Err(AdminKeyError::EmptyFile);
    }

    let mut hasher = Sha256::new();
    hasher.update(key_trimmed.as_bytes());
    let hash: [u8; 32] = hasher.finalize().into();

    Ok(Some(hash))
}

pub async fn admin_auth_middleware(
    State(key_hash): State<Arc<ArcSwap<[u8; 32]>>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let auth_header = req.headers().get("Authorization").and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let provided_hash: [u8; 32] = hasher.finalize().into();

    let stored_hash = key_hash.load();
    if stored_hash.as_ref().ct_eq(&provided_hash).into() {
        next.run(req).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_admin_key_empty_path_returns_none() {
        let result = load_admin_key("").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_admin_key_missing_file_returns_none() {
        let result = load_admin_key("/nonexistent/admin-key").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_admin_key_valid_file_returns_hash() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, "test-secret-key\n").unwrap();

        let result = load_admin_key(key_path.to_str().unwrap()).unwrap();
        assert!(result.is_some());

        let mut hasher = Sha256::new();
        hasher.update(b"test-secret-key");
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn load_admin_key_trims_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, "  test-secret-key  \n").unwrap();

        let result = load_admin_key(key_path.to_str().unwrap()).unwrap();
        assert!(result.is_some());

        let mut hasher = Sha256::new();
        hasher.update(b"test-secret-key");
        let expected: [u8; 32] = hasher.finalize().into();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn load_admin_key_empty_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, "  \n").unwrap();

        let result = load_admin_key(key_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AdminKeyError::EmptyFile));
    }

    #[test]
    fn hash_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, "same-key\n").unwrap();

        let hash1 = load_admin_key(key_path.to_str().unwrap()).unwrap().unwrap();
        let hash2 = load_admin_key(key_path.to_str().unwrap()).unwrap().unwrap();
        assert_eq!(hash1, hash2);
    }
}