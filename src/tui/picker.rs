use crate::types::ModelInfo;

/// State for the model/provider picker modal
#[derive(Debug, Clone)]
pub struct PickerState {
    pub items: Vec<PickerItem>,
    pub selected: usize,
    pub filter: String,
    pub filtering: bool,
}

#[derive(Debug, Clone)]
pub struct PickerItem {
    pub provider_id: String,
    pub provider_name: String,
    pub model_id: String,
    pub model_name: String,
    pub context_window: u32,
    pub cost_display: String,
    pub connected: bool,
}

impl PickerState {
    pub fn new(items: Vec<PickerItem>) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
            filtering: false,
        }
    }

    pub fn filtered_items(&self) -> Vec<&PickerItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let lower = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|item| {
                    item.model_name.to_lowercase().contains(&lower)
                        || item.model_id.to_lowercase().contains(&lower)
                        || item.provider_name.to_lowercase().contains(&lower)
                })
                .collect()
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.filtered_items().len().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    pub fn selected_item(&self) -> Option<&PickerItem> {
        self.filtered_items().get(self.selected).copied()
    }

    pub fn start_filter(&mut self) {
        self.filtering = true;
        self.filter.clear();
    }

    pub fn stop_filter(&mut self) {
        self.filtering = false;
    }

    pub fn add_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
    }

    pub fn remove_filter_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}

impl PickerItem {
    pub fn from_model_info(info: &ModelInfo, connected: bool, provider_name: &str) -> Self {
        let cost = if info.input_cost_per_million == 0.0 && info.output_cost_per_million == 0.0 {
            "Free".to_string()
        } else {
            format!(
                "${:.2}/${:.2}",
                info.input_cost_per_million, info.output_cost_per_million
            )
        };

        let ctx = if info.context_window >= 1_000_000 {
            format!("{}M", info.context_window / 1_000_000)
        } else {
            format!("{}K", info.context_window / 1_000)
        };

        Self {
            provider_id: info.provider_id.clone(),
            provider_name: provider_name.to_string(),
            model_id: info.id.clone(),
            model_name: info.name.clone(),
            context_window: info.context_window,
            cost_display: format!("{ctx} {cost}"),
            connected,
        }
    }
}
