use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;

use crate::config::dynamic_config::DynamicConfig;
use crate::config::static_config::StaticConfig;
use crate::config::validation::validate;

const DEFAULT_CONFIG_PATH: &str = "/etc/reverse-proxy/config.toml";

#[derive(Debug, Parser)]
#[command(name = "reverse-proxy", version, about = "Reverse proxy server")]
pub struct Cli {
    #[arg(long, default_value = DEFAULT_CONFIG_PATH, help = "Path to config file")]
    pub config: String,

    #[arg(long, help = "Validate config and exit")]
    pub validate: bool,

    #[arg(
        long,
        help = "Permit 0.0.0.0 as a bind address (for container deployments)"
    )]
    pub allow_wildcard_bind: bool,
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub static_config: StaticConfig,
    pub dynamic_config: DynamicConfig,
    pub allow_wildcard_bind: bool,
}

pub fn parse() -> Cli {
    Cli::parse()
}

pub fn parse_from<I, S>(args: I) -> Cli
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    Cli::parse_from(args)
}

pub fn load_config(cli: &Cli) -> Result<LoadedConfig> {
    let config_path = Path::new(&cli.config);
    let config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read config file: {}", cli.config))?;

    let full_config = crate::config::FullConfig::parse(&config_content)
        .with_context(|| format!("failed to parse config file: {}", cli.config))?;

    let (static_config, dynamic_config) = full_config.into_static_and_dynamic();

    let allow_wildcard_bind = static_config.allow_wildcard_bind || cli.allow_wildcard_bind;

    validate(&static_config, &dynamic_config, cli.allow_wildcard_bind).map_err(|errors| {
        anyhow::anyhow!(
            "config validation failed:\n{}",
            errors
                .iter()
                .map(|e| format!("  - {}", e))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })?;

    Ok(LoadedConfig {
        static_config,
        dynamic_config,
        allow_wildcard_bind,
    })
}

pub fn run_validate(cli: &Cli) -> Result<()> {
    match load_config(cli) {
        Ok(_) => {
            println!("Configuration is valid.");
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_default_config_path() {
        let cli = Cli::parse_from(["reverse-proxy"]);
        assert_eq!(cli.config, DEFAULT_CONFIG_PATH);
        assert!(!cli.validate);
        assert!(!cli.allow_wildcard_bind);
    }

    #[test]
    fn cli_custom_config_path() {
        let cli = Cli::parse_from(["reverse-proxy", "--config", "/tmp/test.toml"]);
        assert_eq!(cli.config, "/tmp/test.toml");
    }

    #[test]
    fn cli_validate_flag() {
        let cli = Cli::parse_from(["reverse-proxy", "--validate"]);
        assert!(cli.validate);
    }

    #[test]
    fn cli_allow_wildcard_bind_flag() {
        let cli = Cli::parse_from(["reverse-proxy", "--allow-wildcard-bind"]);
        assert!(cli.allow_wildcard_bind);
    }

    #[test]
    fn cli_all_flags() {
        let cli = Cli::parse_from([
            "reverse-proxy",
            "--config",
            "/custom/path.toml",
            "--validate",
            "--allow-wildcard-bind",
        ]);
        assert_eq!(cli.config, "/custom/path.toml");
        assert!(cli.validate);
        assert!(cli.allow_wildcard_bind);
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["reverse-proxy", "--version"]);
        assert!(result.is_err());
        if let Err(err) = result {
            use clap::error::ErrorKind;
            assert!(matches!(err.kind(), ErrorKind::DisplayVersion));
        }
    }

    #[test]
    fn load_config_missing_file() {
        let cli = Cli::parse_from(["reverse-proxy", "--config", "/nonexistent/config.toml"]);
        let result = load_config(&cli);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to read config file") || err_msg.contains("No such file"));
    }

    #[test]
    fn load_config_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "not valid toml {{{{").unwrap();
        let cli = Cli::parse_from(["reverse-proxy", "--config", path.to_str().unwrap()]);
        let result = load_config(&cli);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to parse config file"));
    }

    #[test]
    fn load_config_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, "cert").unwrap();
        std::fs::write(&key_path, "key").unwrap();

        let toml = format!(
            r#"
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
mode = "manual"
cert_path = "{}"
key_path = "{}"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#,
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap()
        );
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();

        let cli = Cli::parse_from(["reverse-proxy", "--config", config_path.to_str().unwrap()]);
        let result = load_config(&cli);
        assert!(result.is_ok());
        let loaded = result.unwrap();
        assert!(!loaded.allow_wildcard_bind);
        assert_eq!(loaded.static_config.listeners.len(), 1);
        assert_eq!(loaded.dynamic_config.sites.len(), 1);
    }

    #[test]
    fn load_config_wildcard_bind_or_logic() {
        let dir = tempfile::tempdir().unwrap();

        let toml = r#"
allow_wildcard_bind = false

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme"
acme_contact = "mailto:admin@test.local"
"#;
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();

        let cli = Cli::parse_from([
            "reverse-proxy",
            "--config",
            config_path.to_str().unwrap(),
            "--allow-wildcard-bind",
        ]);
        let result = load_config(&cli);
        assert!(result.is_ok());
        let loaded = result.unwrap();
        assert!(loaded.allow_wildcard_bind);
    }

    #[test]
    fn load_config_wildcard_bind_rejected_without_flag() {
        let dir = tempfile::tempdir().unwrap();

        let toml = r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme"
acme_contact = "mailto:admin@test.local"
"#;
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();

        let cli = Cli::parse_from(["reverse-proxy", "--config", config_path.to_str().unwrap()]);
        let result = load_config(&cli);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("config validation failed"));
    }

    #[test]
    fn load_config_validation_fails_reports_errors() {
        let dir = tempfile::tempdir().unwrap();

        let toml = r#"
[rate_limit]
requests_per_second = 0
burst = 20

[body]
limit_bytes = 0
"#;
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();

        let cli = Cli::parse_from(["reverse-proxy", "--config", config_path.to_str().unwrap()]);
        let result = load_config(&cli);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("config validation failed"));
    }

    #[test]
    fn load_config_allow_wildcard_from_config_only() {
        let dir = tempfile::tempdir().unwrap();

        let toml = r#"
allow_wildcard_bind = true

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme"
acme_contact = "mailto:admin@test.local"
"#;
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, toml).unwrap();

        let cli = Cli::parse_from(["reverse-proxy", "--config", config_path.to_str().unwrap()]);
        let result = load_config(&cli);
        assert!(result.is_ok());
        assert!(result.unwrap().allow_wildcard_bind);
    }

    #[test]
    fn parse_from_vec() {
        let args = vec!["reverse-proxy", "--validate", "--allow-wildcard-bind"];
        let cli = parse_from(args);
        assert!(cli.validate);
        assert!(cli.allow_wildcard_bind);
    }
}
