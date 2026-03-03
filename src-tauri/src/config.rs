use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoAnonymizeConfig {
    pub browsers: Vec<String>,
    pub ai_assistants: Vec<String>,
    pub custom_apps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub auto_anonymize: AutoAnonymizeConfig,
    /// Language code for PII detection (e.g. "en", "fr"). Default: "en".
    #[serde(default = "Config::default_language")]
    pub language: String,
    /// Presidio confidence score threshold (0.0–1.0). Default: 0.5.
    #[serde(default = "Config::default_score_threshold")]
    pub score_threshold: f64,
}

impl Config {
    fn default_language() -> String {
        "en".to_string()
    }

    fn default_score_threshold() -> f64 {
        0.5
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            score_threshold: 0.5,
            auto_anonymize: AutoAnonymizeConfig {
                browsers: vec![
                    "chrome".to_string(),
                    "firefox".to_string(),
                    "edge".to_string(),
                    "safari".to_string(),
                    "brave".to_string(),
                    "opera".to_string(),
                    "vivaldi".to_string(),
                    "arc".to_string(),
                ],
                ai_assistants: vec![
                    "chatgpt".to_string(),
                    "claude".to_string(),
                    "gemini".to_string(),
                    "copilot".to_string(),
                    "openai".to_string(),
                    "anthropic".to_string(),
                    "bard".to_string(),
                    "perplexity".to_string(),
                    "poe".to_string(),
                ],
                custom_apps: vec![],
            },
        }
    }
}

impl Config {
    /// Load config from file, or create default if it doesn't exist
    pub fn load() -> Self {
        let config_path = Self::get_config_path();

        if let Ok(content) = fs::read_to_string(&config_path) {
            match serde_json::from_str::<Config>(&content) {
                Ok(config) => {
                    log::info!("Loaded config from: {:?}", config_path);
                    return config;
                }
                Err(e) => {
                    log::warn!("Failed to parse config file: {}. Using defaults.", e);
                }
            }
        }

        // If file doesn't exist or parsing failed, create default config
        let default_config = Config::default();

        // Try to save the default config for future use
        if let Err(e) = default_config.save() {
            log::warn!("Failed to save default config: {}", e);
        }

        default_config
    }

    /// Save config to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = Self::get_config_path();

        // Create parent directory if it doesn't exist
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        fs::write(&config_path, json)?;

        log::info!("Saved config to: {:?}", config_path);
        Ok(())
    }

    /// Get the config file path
    fn get_config_path() -> PathBuf {
        // Try to use current directory first (development)
        let dev_path = std::env::current_dir().ok().map(|p| p.join("config.json"));

        if let Some(ref path) = dev_path {
            if path.exists() || std::env::current_dir().is_ok() {
                return path.clone();
            }
        }

        // Fallback to executable directory
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("config.json")))
            .unwrap_or_else(|| PathBuf::from("config.json"))
    }

    /// Get all keywords that should trigger auto-anonymization
    pub fn get_all_keywords(&self) -> Vec<String> {
        let mut keywords = Vec::new();
        keywords.extend(self.auto_anonymize.browsers.clone());
        keywords.extend(self.auto_anonymize.ai_assistants.clone());
        keywords.extend(self.auto_anonymize.custom_apps.clone());
        keywords
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config
            .auto_anonymize
            .browsers
            .contains(&"chrome".to_string()));
        assert!(config
            .auto_anonymize
            .ai_assistants
            .contains(&"chatgpt".to_string()));
    }

    #[test]
    fn test_get_all_keywords() {
        let config = Config::default();
        let keywords = config.get_all_keywords();
        assert!(keywords.contains(&"chrome".to_string()));
        assert!(keywords.contains(&"chatgpt".to_string()));
    }
}
