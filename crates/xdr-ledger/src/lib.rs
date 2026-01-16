use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// Default Budget: $10.00 (in cents/micros if needed, using f64 for simplicity in hackathon)
const DEFAULT_BUDGET: f64 = 10.0; 

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentState {
    pub id: String,
    pub balance_usdc: f64,    // Virtual Wallet
    pub total_spend: f64,     // Metrics
    pub payment_count: u64,   // Metrics
    pub budget_limit: f64,    // Safety Cap
    pub is_active: bool,
}

impl AgentState {
    fn new(id: String) -> Self {
        Self {
            id,
            balance_usdc: 100.0, // Pre-fund mock agents with $100
            total_spend: 0.0,
            payment_count: 0,
            budget_limit: DEFAULT_BUDGET,
            is_active: true,
        }
    }
}

#[derive(Clone, Default)]
pub struct Ledger {
    // DashMap allows concurrent access without locking the whole map
    store: Arc<DashMap<String, AgentState>>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }

    /// Registers an agent if they don't exist. Returns the (potentially new) state.
    pub fn register_or_get(&self, agent_id: &str) -> AgentState {
        // Entry API handles the "check if exists, else insert" atomically
        let entry = self.store.entry(agent_id.to_string()).or_insert_with(|| {
            AgentState::new(agent_id.to_string())
        });
        entry.value().clone()
    }

    pub fn get_state(&self, agent_id: &str) -> Option<AgentState> {
        self.store.get(agent_id).map(|r| r.value().clone())
    }
    
    // We will add debit/credit logic in Stage 4
}