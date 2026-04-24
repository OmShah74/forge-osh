use forge_agent::tui::diff::*;

#[test]
fn format_diff_identifies_additions() {
    let old = "hello\nworld";
    let new = "hello\nbrave\nworld";
    let diff = format_diff(old, new);

    assert!(diff
        .iter()
        .any(|l| l.tag == DiffTag::Added && l.content.contains("brave")));
}

#[test]
fn format_diff_identifies_deletions() {
    let old = "hello\nbrave\nworld";
    let new = "hello\nworld";
    let diff = format_diff(old, new);

    assert!(diff
        .iter()
        .any(|l| l.tag == DiffTag::Removed && l.content.contains("brave")));
}

#[test]
fn unified_diff_generation() {
    let old = "a\nb\nc\n";
    let new = "a\nb2\nc\n";
    let udiff = format_unified_diff("test.txt", old, new);

    assert!(udiff.contains("--- a/test.txt"));
    assert!(udiff.contains("+++ b/test.txt"));
    assert!(udiff.contains("-b"));
    assert!(udiff.contains("+b2"));
}
