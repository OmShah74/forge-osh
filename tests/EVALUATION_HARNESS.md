# Evaluation Harness

`test_evaluation_harness.rs` and `support/eval.rs` provide deterministic
golden tests for agent behavior.

Covered seams:
- mock-provider request capture and scripted responses,
- agent turn history with tool calls and tool results,
- compaction request/persistence behavior,
- long-paste budget classification and clipboard normalization,
- project skill loading and invocation constraints,
- permission decisions and diff-review refusal before file mutation.

Low-space command:

```bash
export PATH="/c/msys64/mingw64/bin:/c/Users/OM SHAH/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="/c/forge-build/target"
cd "/c/Users/OM SHAH/Desktop/forge-osh"
cargo test --test test_evaluation_harness -j 1 -- --test-threads=1
rm -f /c/forge-build/target/debug/deps/test_evaluation_harness-*.exe
```

Add a new golden test here whenever a user-visible regression is fixed.
