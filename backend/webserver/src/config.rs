use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_endpoints: usize,
    pub max_requests_per_minute: usize,
    pub max_response_size_bytes: usize,
    pub max_latency_ms: u64,
}

/// Last known-good commit, and when it was marked stable / last checked.
/// Hardcoded via YAML — updated manually when a new commit is promoted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityConfig {
    pub stable_commit: String,
    pub stable_commit_timestamp: String,
    pub stability_check_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinksConfig {
    pub github: String,
    pub dockerhub: String,
    pub wiki: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub limits: LimitsConfig,
    pub stability: StabilityConfig,
    pub links: LinksConfig,
}

impl AppConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let cfg: AppConfig = serde_yaml::from_str(&contents)?;
        Ok(cfg)
    }

    pub fn from_file_or_default(path: &str) -> Self {
        Self::from_file(path).unwrap_or_else(|_| Self::default())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3000,
            },
            limits: LimitsConfig {
                max_endpoints: 50_000,
                max_requests_per_minute: 100,
                max_response_size_bytes: 10 * 1024 * 1024, // 10 MB
                max_latency_ms: 5_000,
            },
            stability: StabilityConfig {
                stable_commit: "unknown".to_string(),
                stable_commit_timestamp: "unknown".to_string(),
                stability_check_timestamp: "unknown".to_string(),
            },
            links: LinksConfig {
                github: "https://github.com/hashedtokens/edms".to_string(),
                dockerhub: "https://hub.docker.com/r/hashedtokens/edms".to_string(),
                wiki: "https://github.com/hashedtokens/edms/wiki".to_string(),
            },
        }
    }
}
