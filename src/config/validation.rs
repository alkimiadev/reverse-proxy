use anyhow::Result;

use super::dynamic_config::DynamicConfig;
use super::static_config::StaticConfig;

#[allow(dead_code)]
pub fn validate_config(
    _static_config: &StaticConfig,
    _dynamic_config: &DynamicConfig,
) -> Result<()> {
    Ok(())
}
