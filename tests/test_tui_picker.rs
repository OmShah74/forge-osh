use forge_agent::tui::picker::*;
use forge_agent::types::ModelInfo;

#[test]
fn picker_state_initialization() {
    let state = PickerState::new(vec![]);
    assert_eq!(state.selected, 0);
    assert!(!state.filtering);
}

#[test]
fn picker_move_up_down() {
    let model = ModelInfo {
        id: "test".into(),
        name: "Test".into(),
        context_window: 1000,
        supports_tools: true,
        supports_vision: false,
        input_cost_per_million: 0.0,
        output_cost_per_million: 0.0,
        provider_id: "test".into(),
    };
    let item1 = PickerItem::from_model_info(&model, true, "Provider1");
    let item2 = PickerItem::from_model_info(&model, true, "Provider2");

    let mut state = PickerState::new(vec![item1, item2]);
    assert_eq!(state.selected, 0);

    state.move_down();
    assert_eq!(state.selected, 1);

    // Should not exceed bounds
    state.move_down();
    assert_eq!(state.selected, 1);

    state.move_up();
    assert_eq!(state.selected, 0);
}

#[test]
fn picker_filtering_logic() {
    let mut model = ModelInfo {
        id: "test-gpt".into(),
        name: "Test".into(),
        context_window: 1000,
        supports_tools: true,
        supports_vision: false,
        input_cost_per_million: 0.0,
        output_cost_per_million: 0.0,
        provider_id: "test".into(),
    };
    let item1 = PickerItem::from_model_info(&model, true, "OpenAI");
    model.id = "test-claude".into();
    let item2 = PickerItem::from_model_info(&model, true, "Anthropic");

    let mut state = PickerState::new(vec![item1, item2]);

    state.start_filter();
    state.add_filter_char('g');
    state.add_filter_char('p');
    state.add_filter_char('t');

    let filtered = state.filtered_items();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].model_id, "test-gpt");
}
