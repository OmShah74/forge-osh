pub mod checkpoint;
pub mod history;
pub mod tokens;

use serde::{Deserialize, Serialize};

use crate::types::Usage;
use history::ConversationHistory;
use tokens::CostTracker;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model_id: String,
    pub history: ConversationHistory,
    #[serde(skip)]
    pub cost_tracker: CostTracker,
    pub working_dir: String,
}

impl Session {
    pub fn new(name: String, provider_id: String, model_id: String, working_dir: String) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        Self {
            id: id.clone(),
            name,
            provider_id,
            model_id,
            history: ConversationHistory::new(id),
            cost_tracker: CostTracker::new(),
            working_dir,
        }
    }

    /// Record API usage
    pub fn record_usage(&mut self, usage: &Usage, input_cost_per_m: f64, output_cost_per_m: f64) {
        self.cost_tracker.add(usage, input_cost_per_m, output_cost_per_m);
    }

    /// Save session to disk
    pub fn save(&self) -> crate::error::Result<()> {
        checkpoint::Checkpoint::save(self)
    }

    /// Get formatted cost
    pub fn format_cost(&self) -> String {
        self.cost_tracker.format_cost()
    }

    /// Get formatted tokens
    pub fn format_tokens(&self) -> String {
        self.cost_tracker.format_tokens()
    }
}
