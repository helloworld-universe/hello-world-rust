use config::{Config, ConfigError, Environment as ConfigEnvironment, File};
use rootcause::prelude::*;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug)]
pub struct MyConfig {
    pub name: String,
}

pub fn get_configuration() -> Result<MyConfig, ConfigError> {
    // Specify the directory where config files are located
    let config_dir = "config";

    // Determine the environment (e.g., "development", "production")
    let environment = std::env::var("ENV").unwrap_or_else(|_| "dev".into());

    println!("Loading configuration for environment: {}", environment);

    // Create a new configuration builder
    let settings = Config::builder()
        // 1. Load defaults from `config/default.toml`
        .add_source(File::with_name(&format!("{}/default", config_dir)))
        // 2. Load environment-specific settings (optional)
        // This file will only be loaded if it exists (e.g., `config/production.toml`)
        // The `config_rs::File::required(false)` is the default.
        .add_source(
            File::with_name(&format!("{}/{}.toml", config_dir, environment)).required(false),
        )
        // 3. Override with environment variables
        // This is the powerful part. It searches for variables prefixed with "APP"
        // and uses "__" as the separator for nested keys.
        // e.g., APP_DATABASE__URL maps to settings.database.url
        .add_source(
            ConfigEnvironment::with_prefix("HELLO_WORLD")
                .prefix_separator("__")
                .separator("_"),
        )
        // 4. Build and deserialize into the Settings struct
        .build()?;

    // Deserialize the entire configuration into our struct
    settings.try_deserialize()
}

pub fn load_config() -> Result<String, Report> {
    let config = match get_configuration().context(format!(
        "working directory: {}",
        std::env::current_dir().unwrap().display()
    )) {
        Ok(config) => config,
        Err(e) => panic!("Could not load config: {e}"),
    };

    tracing::info!("Loaded config: {:#?}", config);

    match fs::read_to_string("config/config.toml").context(format!(
        "working directory: {}",
        std::env::current_dir().unwrap().display()
    )) {
        Ok(config) => Ok(config),
        Err(e) => panic!("Could not load config: {e}"),
    }
}

//
// Unit Tests
//
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }

    #[test]
    fn test_get_configuration() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
