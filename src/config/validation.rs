use anyhow::{bail, Result};

use super::dynamic_config::DynamicConfig;
use super::static_config::StaticConfig;

#[allow(dead_code)]
pub fn validate_config(
    _static_config: &StaticConfig,
    dynamic_config: &DynamicConfig,
) -> Result<()> {
    if dynamic_config.rate_limit.requests_per_second == 0 {
        bail!("rate_limit.requests_per_second must be > 0");
    }
    if dynamic_config.rate_limit.burst == 0 {
        bail!("rate_limit.burst must be > 0");
    }
    if dynamic_config.body.limit_bytes == 0 {
        bail!("body.limit_bytes must be > 0");
    }
    for site in &dynamic_config.sites {
        if site.host.is_empty() {
            bail!("site host must not be empty");
        }
        if site.upstream.is_empty() {
            bail!("site upstream must not be empty");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_fixtures;

    #[test]
    fn valid_config_passes_validation() {
        let static_config = test_fixtures::test_static_config();
        let dynamic_config = test_fixtures::test_dynamic_config();
        assert!(validate_config(&static_config, &dynamic_config).is_ok());
    }

    #[test]
    fn zero_requests_per_second_fails() {
        let static_config = test_fixtures::test_static_config();
        let mut dynamic_config = test_fixtures::test_dynamic_config();
        dynamic_config.rate_limit.requests_per_second = 0;
        assert!(validate_config(&static_config, &dynamic_config).is_err());
    }

    #[test]
    fn zero_burst_fails() {
        let static_config = test_fixtures::test_static_config();
        let mut dynamic_config = test_fixtures::test_dynamic_config();
        dynamic_config.rate_limit.burst = 0;
        assert!(validate_config(&static_config, &dynamic_config).is_err());
    }

    #[test]
    fn zero_body_limit_fails() {
        let static_config = test_fixtures::test_static_config();
        let mut dynamic_config = test_fixtures::test_dynamic_config();
        dynamic_config.body.limit_bytes = 0;
        assert!(validate_config(&static_config, &dynamic_config).is_err());
    }
}
