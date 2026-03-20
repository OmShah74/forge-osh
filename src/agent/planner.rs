/// Optional planning phase for complex tasks.
/// When planning_mode is enabled, the agent will first describe its plan
/// before executing.

pub struct Planner;

impl Planner {
    /// Heuristic: determine if a task is complex enough to warrant planning
    pub fn is_complex_task(message: &str) -> bool {
        let complex_indicators = [
            "refactor",
            "migrate",
            "build",
            "create",
            "implement",
            "set up",
            "setup",
            "redesign",
            "add feature",
            "new feature",
            "multi-file",
            "multiple files",
            "architecture",
            "rewrite",
            "overhaul",
        ];

        let word_count = message.split_whitespace().count();
        if word_count > 30 {
            return true;
        }

        let lower = message.to_lowercase();
        complex_indicators
            .iter()
            .any(|indicator| lower.contains(indicator))
    }

    /// Generate a planning prompt to prepend before executing
    pub fn planning_prompt(user_message: &str) -> String {
        format!(
            r#"The user has requested a complex task. Before executing, provide a brief plan:

1. List the high-level steps you'll take (3-7 steps)
2. Identify which files you'll need to read or modify
3. Note any potential risks or things you'll need to be careful about

User's request: {user_message}

Provide your plan, then proceed to execute it step by step."#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complex_detection() {
        assert!(Planner::is_complex_task("Refactor the auth module to use JWT"));
        assert!(Planner::is_complex_task("Build a new REST API with CRUD operations"));
        assert!(!Planner::is_complex_task("What does this function do?"));
        assert!(!Planner::is_complex_task("Fix the typo"));
    }
}
