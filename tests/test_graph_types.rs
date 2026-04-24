use forge_agent::graph::types::*;

#[test]
fn language_from_extension() {
    assert_eq!(Language::from_extension("rs"), Language::Rust);
    assert_eq!(Language::from_extension("py"), Language::Python);
    assert_eq!(Language::from_extension("js"), Language::JavaScript);
    assert_eq!(Language::from_extension("go"), Language::Go);
    assert_eq!(Language::from_extension("xyz123"), Language::Unknown);
}

#[test]
fn modifiers_bitwise_flags() {
    let mut mods = Modifiers::default();
    assert!(!mods.is_public());

    mods.set(mflags::IS_PUBLIC);
    assert!(mods.is_public());

    mods = mods.with(mflags::IS_ASYNC);
    assert!(mods.is_async());
}

#[test]
fn code_content_generation() {
    let code = "fn main() {\n    println!(\"hello\");\n}\n";
    let content = CodeContent::new(code.to_string());

    assert_eq!(content.signature_only, "fn main()");
    assert!(content.token_weight > 0);
}
