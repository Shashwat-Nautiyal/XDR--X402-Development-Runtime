use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosConfig {
    pub enabled: bool,
    pub seed: u64,
    pub global_failure_rate: f64,  // General network failure (503/429)
    pub payment_failure_rate: f64, // Payment tx fails (on chain)
    pub rug_rate: f64,             // Payment succeeds, but request fails (Lost funds)
    pub min_latency_ms: u64,
    pub max_latency_ms: u64,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            seed: 0,
            global_failure_rate: 0.0,
            payment_failure_rate: 0.0,
            rug_rate: 0.0,
            min_latency_ms: 0,
            max_latency_ms: 0,
        }
    }
}

#[derive(Clone)]
pub struct ChaosEngine {
    // We use Mutex instead of RwLock because checking chaos MODIFIES the RNG state
    state: Arc<Mutex<ChaosState>>,
}

struct ChaosState {
    config: ChaosConfig,
    rng: ChaCha8Rng,
}

impl Default for ChaosEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ChaosEngine {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ChaosState {
                config: ChaosConfig::default(),
                rng: ChaCha8Rng::seed_from_u64(0),
            })),
        }
    }

    pub fn set_config(&self, new_config: ChaosConfig) {
        let mut state = self.state.lock().unwrap();
        // Re-seed the RNG whenever config changes to ensure replayability from this point
        state.rng = ChaCha8Rng::seed_from_u64(new_config.seed);
        state.config = new_config;
        info!("Chaos Re-Seeded & Updated: {:?}", state.config);
    }

    /// Returns a clone of the current chaos configuration (for TUI display)
    pub fn get_config(&self) -> ChaosConfig {
        let state = self.state.lock().unwrap();
        state.config.clone()
    }

    /// Roll dice for generic network failure (503/429)
    pub fn roll_network_failure(&self) -> Option<u16> {
        let mut state = self.state.lock().unwrap();
        
        // Extract boolean first
        let enabled = state.config.enabled;
        let rate = state.config.global_failure_rate;

        if !enabled { return None; }

        // Now mutate RNG
        if state.rng.gen_bool(rate) {
            let errors = [503, 429, 504];
            let idx = state.rng.gen_range(0..errors.len());
            return Some(errors[idx]);
        }
        None
    }

    /// Roll dice for payment processing failure (Payment Rejected)
    pub fn roll_payment_failure(&self) -> bool {
        let mut state = self.state.lock().unwrap();
       // 1. EXTRACT VALUES (Read Borrow)
        let enabled = state.config.enabled;
        let rate = state.config.payment_failure_rate;

        if !enabled { return false; }

        // 2. MUTATE RNG (Write Borrow)
        // Now we pass the COPY 'rate', not the borrow 'state.config.rate'
        state.rng.gen_bool(rate)
    }

    /// Roll dice for "Rug" (Payment Accepted -> Request Failed)
    pub fn roll_rug_pull(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let enabled = state.config.enabled;
        let rate = state.config.rug_rate;

        if !enabled { return false; }

        // 2. MUTATE RNG (Write Borrow)
        state.rng.gen_bool(rate)
    }

    pub async fn inject_latency(&self) {
        let (enabled, delay) = {
            let mut state = self.state.lock().unwrap();
            
            // Extract values first (READ)
            let enabled = state.config.enabled;
            let min = state.config.min_latency_ms;
            let max = state.config.max_latency_ms;

            if !enabled || max == 0 {
                (false, 0)
            } else {
                // Now mutate RNG (WRITE)
                // We use the local 'min'/'max' copies, so we don't touch 'state.config' here
                (true, state.rng.gen_range(min..=max))
            }
        };

        if enabled && delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }
}