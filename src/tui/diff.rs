use similar::{ChangeTag, TextDiff};

/// Generate a colored diff display between old and new content
pub fn format_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();

    for change in diff.iter_all_changes() {
        let (tag, prefix) = match change.tag() {
            ChangeTag::Delete => (DiffTag::Removed, "-"),
            ChangeTag::Insert => (DiffTag::Added, "+"),
            ChangeTag::Equal => (DiffTag::Context, " "),
        };

        lines.push(DiffLine {
            tag,
            content: format!("{prefix} {}", change.value().trim_end_matches('\n')),
        });
    }

    lines
}

/// Generate a unified diff header
pub fn format_unified_diff(path: &str, old: &str, new: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();

    output.push_str(&format!("--- a/{path}\n"));
    output.push_str(&format!("+++ b/{path}\n"));

    for hunk in diff.unified_diff().header("", "").iter_hunks() {
        output.push_str(&hunk.to_string());
    }

    output
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffTag {
    Added,
    Removed,
    Context,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub tag: DiffTag,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_diff() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nmodified\nline3\n";
        let lines = format_diff(old, new);

        assert!(lines.iter().any(|l| l.tag == DiffTag::Removed));
        assert!(lines.iter().any(|l| l.tag == DiffTag::Added));
        assert!(lines.iter().any(|l| l.tag == DiffTag::Context));
    }
}
