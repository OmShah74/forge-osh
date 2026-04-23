pub mod checkpoint;
pub mod file_cache;
pub mod history;
pub mod tokens;

pub use file_cache::FileStateCache;

use serde::{Deserialize, Serialize};

use crate::skills::{ActiveSkillScope, SkillInvocationRecord};
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
    #[serde(default)]
    pub cost_tracker: CostTracker,
    pub working_dir: String,
    /// Effort level 1–5. Maps to a temperature override in the agent loop.
    /// 1 = minimal/deterministic, 3 = balanced (default), 5 = maximum/creative.
    #[serde(default = "default_effort")]
    pub effort_level: u8,
    #[serde(default)]
    pub invoked_skills: Vec<SkillInvocationRecord>,
    #[serde(default)]
    pub active_skill_scope: Option<ActiveSkillScope>,
}

fn default_effort() -> u8 {
    3
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
            effort_level: 3,
            invoked_skills: Vec::new(),
            active_skill_scope: None,
        }
    }

    /// Record API usage
    pub fn record_usage(&mut self, usage: &Usage, input_cost_per_m: f64, output_cost_per_m: f64) {
        self.cost_tracker
            .add(usage, input_cost_per_m, output_cost_per_m);
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

    pub fn push_invoked_skill(&mut self, record: SkillInvocationRecord) {
        self.invoked_skills.push(record);
        if self.invoked_skills.len() > 32 {
            let drop_count = self.invoked_skills.len().saturating_sub(32);
            self.invoked_skills.drain(0..drop_count);
        }
    }

    pub fn active_skill_scope(&self) -> Option<&ActiveSkillScope> {
        self.active_skill_scope.as_ref()
    }
}
