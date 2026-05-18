//! Goal-policy evaluator.
//!
//! In phase 3 the worker runs under `PermissionMode::Default` and a small
//! responder task drains every `PermissionRequest` the agent loop emits.
//! For each request we call [`evaluate`] against the goal's [`Policy`] and
//! answer Allow/Deny automatically — the user is never prompted.
//!
//! Note on path-glob matching: `PermissionRequest` only carries an
//! `input_summary` (a human-readable blurb) rather than the raw JSON args,
//! so this evaluator uses substring matching against the summary for
//! deny-glob enforcement. Path-glob write filtering is therefore advisory
//! in phase 3 — it kicks in for `Mutating` calls when the summary contains
//! a clear path token. Phase 4 plumbs the raw args through so the gate can
//! be exact.

use crate::types::PermissionLevel;

use super::{AutoApprove, Policy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(String),
}

/// Exact evaluator — operates on the raw JSON args of the tool call.
/// Path-glob enforcement uses every path-typed arg explicitly (no
/// heuristics). Falls back to the heuristic [`evaluate`] when no
/// path-typed args are extractable, which keeps shell command matching
/// (which lives only in `input_summary`) working.
pub fn evaluate_with_args(
    tool_name: &str,
    input_summary: &str,
    args: &serde_json::Value,
    level: &PermissionLevel,
    policy: &Policy,
) -> Decision {
    let paths = extract_paths_from_args(args);

    // Hard deny: any extracted path matching a deny_glob is a hard fail.
    if let Some(hit) = first_glob_match(&paths, &policy.deny_globs) {
        return Decision::Deny(format!("path matches deny_glob '{hit}'"));
    }

    // MCP tools — trust-tagged at registration; allow unless ReadOnly.
    if tool_name.starts_with("mcp__") {
        return match policy.auto_approve {
            AutoApprove::ReadOnly => Decision::Deny(
                "ReadOnly policy: MCP tools blocked (they usually mutate external state)"
                    .into(),
            ),
            _ => Decision::Allow,
        };
    }

    if policy.auto_approve == AutoApprove::All {
        return Decision::Allow;
    }

    match level {
        PermissionLevel::ReadOnly => Decision::Allow,

        PermissionLevel::Network => {
            if policy.network {
                Decision::Allow
            } else {
                Decision::Deny("policy.network = false".into())
            }
        }

        PermissionLevel::Mutating => match policy.auto_approve {
            AutoApprove::ReadOnly => {
                Decision::Deny("ReadOnly policy: mutating tools blocked".into())
            }
            AutoApprove::AllowedTools => {
                if policy.write_globs.is_empty() {
                    return Decision::Allow;
                }
                if paths.is_empty() {
                    // No raw path arg found; fall back to summary heuristic.
                    return evaluate(tool_name, input_summary, level, policy);
                }
                // Every referenced path must match at least one write_glob.
                for p in &paths {
                    if !any_glob_match(p, &policy.write_globs) {
                        return Decision::Deny(format!(
                            "path '{p}' is not covered by any write_glob in {:?}",
                            policy.write_globs
                        ));
                    }
                }
                Decision::Allow
            }
            AutoApprove::All => Decision::Allow,
        },

        PermissionLevel::Destructive => match policy.auto_approve {
            AutoApprove::All => Decision::Allow,
            _ => Decision::Deny(
                "destructive tools require auto_approve = all (explicit opt-in)".into(),
            ),
        },

        PermissionLevel::Shell => {
            // Shell commands aren't in `paths` — fall back to the summary
            // evaluator, which knows how to extract `cmd:` and match the
            // allowlist regex.
            evaluate(tool_name, input_summary, level, policy)
        }
    }
}

/// Extract every path-typed string from a JSON args object. Handles the
/// common keys (`path`, `file_path`, `filename`, `filepath`, `dir`,
/// `target`, `dst`, `src`) and arrays under `paths` / `files`.
fn extract_paths_from_args(args: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    let scalar_keys = [
        "path",
        "file_path",
        "filename",
        "filepath",
        "dir",
        "directory",
        "target",
        "target_file",
        "src",
        "source",
        "dst",
        "dest",
        "destination",
    ];
    let array_keys = ["paths", "files", "targets"];
    if let Some(obj) = args.as_object() {
        for k in scalar_keys {
            if let Some(v) = obj.get(k).and_then(|x| x.as_str()) {
                out.push(v.to_string());
            }
        }
        for k in array_keys {
            if let Some(arr) = obj.get(k).and_then(|x| x.as_array()) {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }
    }
    out
}

fn first_glob_match<'a>(paths: &[String], globs: &'a [String]) -> Option<&'a str> {
    for p in paths {
        for g in globs {
            if let Ok(pat) = glob::Pattern::new(g) {
                if pat.matches(p) {
                    return Some(g);
                }
            }
        }
    }
    None
}

fn any_glob_match(path: &str, globs: &[String]) -> bool {
    for g in globs {
        if let Ok(pat) = glob::Pattern::new(g) {
            if pat.matches(path) {
                return true;
            }
        }
    }
    false
}

/// Evaluate a single permission request using summary heuristics only.
/// Phase 3 entry point — kept as a fallback for [`evaluate_with_args`].
pub fn evaluate(
    tool_name: &str,
    input_summary: &str,
    level: &PermissionLevel,
    policy: &Policy,
) -> Decision {
    // Hard global deny — matches `deny_globs` regardless of mode. The
    // built-in defaults always cover `.git/**`, `**/keys.json`, `**/.env`.
    if let Some(hit) = mentions_any_deny(input_summary, &policy.deny_globs) {
        return Decision::Deny(format!("path matches deny_glob '{hit}'"));
    }

    // MCP tools — already trust-tagged at registration. Allow unless mode
    // is the strictest ReadOnly.
    if tool_name.starts_with("mcp__") {
        return match policy.auto_approve {
            AutoApprove::ReadOnly => Decision::Deny(
                "ReadOnly policy: MCP tools (which usually mutate external state) blocked".into(),
            ),
            _ => Decision::Allow,
        };
    }

    // AutoApprove::All — every internal level allowed (deny_glob already filtered).
    if policy.auto_approve == AutoApprove::All {
        return Decision::Allow;
    }

    match level {
        PermissionLevel::ReadOnly => Decision::Allow,

        PermissionLevel::Network => {
            if policy.network {
                Decision::Allow
            } else {
                Decision::Deny("policy.network = false".into())
            }
        }

        PermissionLevel::Mutating => match policy.auto_approve {
            AutoApprove::ReadOnly => {
                Decision::Deny("ReadOnly policy: mutating tools blocked".into())
            }
            AutoApprove::AllowedTools => {
                // If write_globs is non-empty, check the summary for any
                // referenced path; require ≥1 match. Empty write_globs
                // defaults to "**" (workdir is writable) per the design.
                if policy.write_globs.is_empty() {
                    Decision::Allow
                } else if write_glob_matches(input_summary, &policy.write_globs) {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "no write_glob in {:?} matches the affected path",
                        policy.write_globs
                    ))
                }
            }
            AutoApprove::All => Decision::Allow,
        },

        PermissionLevel::Destructive => match policy.auto_approve {
            AutoApprove::All => Decision::Allow,
            _ => Decision::Deny(
                "destructive tools require auto_approve = all (explicit opt-in)".into(),
            ),
        },

        PermissionLevel::Shell => match policy.auto_approve {
            AutoApprove::ReadOnly => {
                Decision::Deny("ReadOnly policy: shell tools blocked".into())
            }
            AutoApprove::All => Decision::Allow,
            AutoApprove::AllowedTools => {
                if shell_allowlist_match(input_summary, &policy.shell_allowlist) {
                    Decision::Allow
                } else {
                    Decision::Deny(format!(
                        "shell command does not match any allowlist regex {:?}",
                        policy.shell_allowlist
                    ))
                }
            }
        },
    }
}

fn shell_allowlist_match(summary: &str, allowlist: &[String]) -> bool {
    let cmd = extract_shell_cmd(summary);
    let probe = cmd.unwrap_or(summary);
    for pat in allowlist {
        if let Ok(re) = regex::Regex::new(pat) {
            if re.is_match(probe) {
                return true;
            }
        }
    }
    false
}

/// `PermissionRequest::input_summary` is human-readable. For shell tools it
/// commonly looks like `bash: cargo test --release` or `command: ls -la`.
/// We strip the leading label so the allowlist regex can anchor on the
/// real command. If we cannot find a clear separator, fall back to the
/// whole summary.
fn extract_shell_cmd(summary: &str) -> Option<&str> {
    let trimmed = summary.trim();
    let lower = trimmed.to_ascii_lowercase();
    for label in [
        "command:",
        "cmd:",
        "bash:",
        "shell:",
        "running:",
        "exec:",
    ] {
        if let Some(idx) = lower.find(label) {
            let after = &trimmed[idx + label.len()..];
            return Some(after.trim_start());
        }
    }
    None
}

fn write_glob_matches(summary: &str, globs: &[String]) -> bool {
    // Conservative: try to extract a path-looking token from the summary,
    // match each glob via `glob::Pattern`. If no path-looking token is
    // found, allow (the model may be using a tool with no obvious path).
    let candidate = extract_path_token(summary);
    let probe = match candidate {
        Some(p) => p,
        None => return true,
    };
    for g in globs {
        if let Ok(pat) = glob::Pattern::new(g) {
            if pat.matches(probe) {
                return true;
            }
        }
    }
    false
}

fn mentions_any_deny<'a>(summary: &str, deny_globs: &'a [String]) -> Option<&'a str> {
    let candidate = extract_path_token(summary);
    let probe = match candidate {
        Some(p) => p,
        None => return None,
    };
    for g in deny_globs {
        if let Ok(pat) = glob::Pattern::new(g) {
            if pat.matches(probe) {
                return Some(g);
            }
        }
    }
    None
}

/// Heuristic: pull the first whitespace-separated token that looks like a
/// path (contains a `/`, `\\`, `.`, or matches a known directory prefix).
fn extract_path_token(summary: &str) -> Option<&str> {
    for tok in summary.split_whitespace() {
        let cleaned = tok
            .trim_matches(|c: char| c == '"' || c == '\'' || c == ',' || c == '(' || c == ')');
        let looks_like_path = cleaned.contains('/')
            || cleaned.contains('\\')
            || cleaned.ends_with(".rs")
            || cleaned.ends_with(".toml")
            || cleaned.ends_with(".md")
            || cleaned.ends_with(".txt")
            || cleaned.ends_with(".json")
            || cleaned.starts_with("./")
            || cleaned.starts_with("src")
            || cleaned.starts_with("tests");
        if looks_like_path {
            return Some(cleaned);
        }
    }
    None
}
