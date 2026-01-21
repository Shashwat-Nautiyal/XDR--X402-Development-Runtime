use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_BUDGET: f64 = 10.0; 

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentState {
    pub id: String,
    pub balance_usdc: f64,
    pub total_spend: f64,
    pub payment_count: u64,
    pub budget_limit: f64,
    pub is_active: bool,
}

impl AgentState {
    fn new(id: String) -> Self {
        Self {
            id,
            balance_usdc: 100.0, // Free 100 mock USDC
            total_spend: 0.0,
            payment_count: 0,
            budget_limit: DEFAULT_BUDGET,
            is_active: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id: String,
    pub amount: f64,
    pub is_paid: bool,
    pub agent_id: String,
}

#[derive(Clone, Default)]
pub struct Ledger {
    store: Arc<DashMap<String, AgentState>>,
    invoices: Arc<DashMap<String, Invoice>>,
}

impl Ledger {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
            invoices: Arc::new(DashMap::new()),
        }
    }

    pub fn register_or_get(&self, agent_id: &str) -> AgentState {
        let entry = self.store.entry(agent_id.to_string()).or_insert_with(|| {
            AgentState::new(agent_id.to_string())
        });
        entry.value().clone()
    }

    pub fn get_state(&self, agent_id: &str) -> Option<AgentState> {
        self.store.get(agent_id).map(|r| r.value().clone())
    }

    /// Creates a new pending invoice
    pub fn create_invoice(&self, agent_id: &str, amount: f64) -> Invoice {
        let id = Uuid::new_v4().to_string();
        let invoice = Invoice {
            id: id.clone(),
            amount,
            is_paid: false,
            agent_id: agent_id.to_string(),
        };
        self.invoices.insert(id.clone(), invoice.clone());
        invoice
    }

    // Admin function to force-set a balance (for testing exhaustion)
    pub fn set_balance(&self, agent_id: &str, amount: f64) {
        let mut entry = self.store.entry(agent_id.to_string()).or_insert_with(|| {
            AgentState::new(agent_id.to_string())
        });
        entry.balance_usdc = amount;
    }

    pub fn pay_invoice(&self, invoice_id: &str, agent_id: &str) -> Result<f64, String> {
        // 1. Validate Invoice
        let mut invoice = self.invoices.get_mut(invoice_id).ok_or("Invoice invalid")?;
        
        if invoice.is_paid {
            return Err("Invoice already paid".to_string());
        }
        if invoice.agent_id != agent_id {
            return Err("Invoice belongs to another agent".to_string());
        }

        // 2. Validate Funds & Safety
        let mut agent = self.store.get_mut(agent_id).ok_or("Agent not found")?;
        
        // CHECK 1: Wallet Balance
        if agent.balance_usdc < invoice.amount {
            return Err("Wallet Exhausted: Insufficient funds".to_string());
        }
        
        // CHECK 2: Safety Budget (Total Spend Cap)
        if (agent.total_spend + invoice.amount) > agent.budget_limit {
            return Err("Safety Limit: Budget cap exceeded".to_string());
        }

        // 3. Execute
        agent.balance_usdc -= invoice.amount;
        agent.total_spend += invoice.amount;
        agent.payment_count += 1;
        
        invoice.is_paid = true;

        Ok(agent.balance_usdc)
    }
}