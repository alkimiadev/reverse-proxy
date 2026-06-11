use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct DynamicConfig {
    pub sites: Vec<SiteConfig>,
    pub rate_limit: RateLimitConfig,
    pub body: BodyConfig,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct SiteConfig {
    pub host: String,
    pub upstream: String,
    #[serde(default = "default_upstream_scheme")]
    pub upstream_scheme: String,
    #[serde(default = "default_connect_timeout")]
    pub upstream_connect_timeout_secs: u64,
    #[serde(default = "default_request_timeout")]
    pub upstream_request_timeout_secs: u64,
}

#[allow(dead_code)]
fn default_upstream_scheme() -> String {
    "http".to_string()
}

#[allow(dead_code)]
fn default_connect_timeout() -> u64 {
    5
}

#[allow(dead_code)]
fn default_request_timeout() -> u64 {
    60
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
pub struct BodyConfig {
    pub limit_bytes: u64,
}
