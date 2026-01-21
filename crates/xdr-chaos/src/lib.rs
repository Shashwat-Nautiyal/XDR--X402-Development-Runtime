use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosConfig {
    pub enabled: bool,
    pub failure_rate: f64, // 0.0 to 1.0 (e.g., 0.2 = 20% failure)
    pub min_latency_ms: u64,
    pub max_latency_ms: u64,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            failure_rate: 0.2, // Default 20% failure when enabled
            min_latency_ms: 500,
            max_latency_ms: 2000,
        }
    }
}

#[derive(Clone, Default)]
pub struct ChaosEngine {
    // RwLock allows many readers (requests) but one writer (admin CLI)
    config: Arc<RwLock<ChaosConfig>>,
}

impl ChaosEngine {
    pub fn new() -> Self {
        Self {
            config: Arc::new(RwLock::new(ChaosConfig::default())),
        }
    }

    pub fn set_config(&self, new_config: ChaosConfig) {
        let mut write_guard = self.config.write().unwrap();
        *write_guard = new_config;
        info!("🌪️ Chaos Configuration Updated: {:?}", *write_guard);
    }

    pub fn get_config(&self) -> ChaosConfig {
        self.config.read().unwrap().clone()
    }

    /// Returns a random latency duration if chaos is enabled
    pub async fn inject_latency(&self) -> Option<Duration> {
        let config = self.config.read().unwrap();
        if !config.enabled {
            return None;
        }

        let mut rng = rand::thread_rng();
        let ms = rng.gen_range(config.min_latency_ms..=config.max_latency_ms);
        Some(Duration::from_millis(ms))
    }

    /// Returns an Error Status Code if the dice roll fails
    pub fn inject_failure(&self) -> Option<u16> {
        let config = self.config.read().unwrap();
        if !config.enabled {
            return None;
        }

        let mut rng = rand::thread_rng();
        if rng.gen_bool(config.failure_rate) {
            // Pick a random "Blockchain" error
            let errors = [
                503, // Service Unavailable (Node down)
                429, // Too Many Requests (Rate limit)
                504, // Gateway Timeout (Consensus stuck)
            ];
            let idx = rng.gen_range(0..errors.len());
            return Some(errors[idx]);
        }
        None
    }
}