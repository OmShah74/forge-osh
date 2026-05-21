# forge-osh — Production-Grade Benchmarking Plan, Costs, and Affordable Alternatives

> Companion document to `scaling.md`.
> Purpose: give a realistic, honest answer to "how do we benchmark forge-osh like a serious project (SWE-Bench, Terminal-Bench, etc.) without it becoming a financial burden?"
> Audience: solo maintainer on a personal-project budget.

---

## 1. The Major Benchmarks — What They Actually Are

| Benchmark | What it tests | Tasks | Runtime / infra requirement | Notes |
|---|---|---|---|---|
| **SWE-Bench Verified** | Real GitHub issues from 12 Python repos; agent must produce a patch that passes hidden tests | **500** human-verified tasks (subset of 2,294 full) | Docker per task (each repo has a pinned env), ~5–15 min/task, ~16 GB RAM safe, ~50 GB disk for cached images | The industry standard. "Verified" is the right slice — full SWE-Bench has noise. |
| **SWE-Bench Lite** | Easier subset, single-file fixes | **300** tasks | Same Docker setup, ~3–8 min/task | Cheaper warm-up. What Aider uses. |
| **SWE-Bench Multimodal** | Adds screenshots/PDFs | 517 tasks | Same + image input | Skip unless multimodal input ships first. |
| **Terminal-Bench** (Stanford, 2024) | Agent must complete terminal tasks (compile, debug, deploy, sysadmin) | **80+** tasks | Docker per task; tasks include kernel building, distributed setup | Hard — tests shell-agent skill, not coding skill. Closest match to what forge-osh actually is. |
| **Aider Polyglot Benchmark** | 225 Exercism exercises across 6 languages | **225** tasks | Just needs the language toolchains; no Docker required | Cheapest serious benchmark. Aider publishes a leaderboard you can compare to. |
| **LiveCodeBench** | Competitive-programming problems pulled monthly to avoid contamination | ~400 tasks | Lightweight Python sandbox | Tests raw code-gen, not agentic behavior. |
| **HumanEval+ / MBPP+** | Function-completion | 164 / 378 | Trivial sandbox | Too easy for an agent — saturates. |
| **MLE-Bench (Kaggle)** | ML engineering tasks | 75 | Heavy (GPU often required) | Skip; not your audience. |
| **WebArena / VisualWebArena** | Browser agent tasks | ~800 | Needs a browser stack | Skip — forge-osh is terminal-focused. |
| **τ-bench (tau-bench)** | Tool-use accuracy in retail/airline domains | ~165 | Trivial — just an API mock | Cheap. Tests tool reliability, which IS your area. |
| **BigCodeBench** | Practical coding with library calls | 1,140 tasks | Lightweight sandbox | Good middle ground. |

---

## 2. Realistic Cost Breakdown

Mid-2026 list prices for plausibly-tested models. Input/output ratios are taken from real agent traces — agents are input-heavy because of file-read tool returns.

**Assumptions for a single SWE-Bench-Verified run (one model, all 500 tasks):**
- Avg per-task: ~80 turns, ~250k input tokens (cached after first turn), ~20k output tokens.
- With **prompt caching working**: effective input ≈ 60k uncached + 190k cached.
- Without caching: full 250k uncached.

### Per-task API cost (Claude Sonnet 4.6, with caching)
- 60k × $3/M + 190k × $0.30/M + 20k × $15/M = **$0.54/task**
- × 500 tasks = **~$270 per full SWE-Bench-Verified run on Sonnet**

### Per-task API cost without caching (current state of forge-osh)
- 250k × $3/M + 20k × $15/M = **$1.05/task**
- × 500 tasks = **~$525 per full run**

### Per-model, per-benchmark estimates (with caching enabled)

| Model | SWE-Bench Verified (500) | SWE-Bench Lite (300) | Terminal-Bench (80) | Aider Polyglot (225) | τ-bench (165) |
|---|---|---|---|---|---|
| **Claude Sonnet 4.6** | $270 | $130 | $50 | $25 | $15 |
| **Claude Opus 4.7** | $1,350 | $650 | $250 | $125 | $75 |
| **GPT-5 / GPT-4o-class** | $200 | $95 | $40 | $20 | $12 |
| **GPT-5-mini** | $40 | $20 | $8 | $4 | $3 |
| **Gemini 2.5 Pro** | $150 | $70 | $30 | $15 | $10 |
| **DeepSeek V3 / R1** | $25 | $12 | $5 | $3 | $2 |
| **Groq Llama 3.3 70B** | $15 | $8 | $3 | $2 | $1 |
| **Local Ollama (Qwen 2.5 Coder 32B)** | $0 (electricity ~$2) | $0 | $0 | $0 | $0 |

### Total for a "publish a leaderboard" run (5 frontier models × SWE-Bench-Verified)
- With caching: **~$2,000–2,500**
- Without caching: **~$4,000–5,000**

### Compute infra cost (Docker per task)
- Locally on your dev box: **$0**, but ~25–50 hours wall-time per 500-task run (single-threaded). Parallel 8-wide: 3–6 hours but spikes RAM/disk.
- Cloud (Hetzner CCX33, 8 vCPU / 32 GB / 360 GB): **€0.10/hour** ≈ $5–10 per full SWE-Bench run.
- AWS m6i.2xlarge: ~$0.40/hr = $20–40 per run. More than needed.

**Total realistic outlay to publish a serious benchmark**: ~$2,500–3,500 for the API + $50 infra + ~80 hours of your time over 2–3 weeks. That's the **production-grade tier**.

---

## 3. Complexity Cost (Engineering Time)

| Item | Time |
|---|---|
| Wire SWE-Bench harness (already MIT — `princeton-nlp/SWE-bench`) | 1 week |
| Adapter from forge-osh's `--output-format=stream-json` (needs §3.5 from `scaling.md`) → SWE-Bench prediction format | 2–3 days |
| Docker-per-task orchestration + retry logic | 4 days |
| Terminal-Bench adapter (different format) | 3 days |
| Aider Polyglot adapter (simplest — just Exercism scaffolding) | 2 days |
| τ-bench (simplest of all — JSON tool-use scoring) | 1 day |
| Result aggregation + CSV/Markdown dashboard | 2 days |
| Caching/retry/resumability (a bad model can hang for hours — you need timeouts) | 3 days |
| **Total** | **~3 weeks** of focused work |

This is feasible for one developer. It is **not** feasible to do well as a side-project across 6 months.

---

## 4. The Honest Recommendation — A 3-Tier Strategy

Pick the tier that matches your wallet and goals.

### Tier 1 — "Free / near-free, ship in 1 week" (RECOMMENDED)

**Goal:** Have a credible benchmark you can put in the README, without spending more than $100.

**What you run:**
1. **Aider Polyglot (225 tasks)** — `$2–4` per run with DeepSeek/Groq, $20–25 with Sonnet. Run once each on **3 models** (Sonnet, DeepSeek-V3, local Qwen 2.5 Coder via Ollama). Total: **~$30**.
2. **τ-bench retail (115 tasks)** — `$10` on Sonnet, free on local. Total: **~$15**.
3. **SWE-Bench-Lite — 50-task random subset (not the full 300)** — `$10` on DeepSeek, $25 on Sonnet. Total: **~$40**.
4. **A handful (10–20) of Terminal-Bench tasks**, picked to cover the categories you care about. **~$10**.

**Total: ~$100. Engineering: 1 week.** Results are publishable; you label them "subset" honestly.

This is what Aider, Cline, and most open-source agents actually do. Nobody runs the full SWE-Bench on every commit — it's run once before a release announcement.

### Tier 2 — "Credible 'we benchmarked properly' tier, ~$500"

**Goal:** Real comparison against Claude Code, Codex CLI, Cursor.

- Full **Aider Polyglot** on 5 models: **~$80**
- Full **SWE-Bench-Lite (300)** on 3 models (Sonnet, DeepSeek, GPT-5-mini): **~$170**
- Full **τ-bench (165)** on 3 models: **~$40**
- Sampled **Terminal-Bench (40 tasks)** on 3 models: **~$60**
- Buffer for retries / debugging: **~$150**

**Total: ~$500. Engineering: 2 weeks.**

### Tier 3 — "Publish on the official leaderboard, ~$3,000"

Full SWE-Bench Verified on 5+ models. Only do this for a v2.0 launch. Recoup via Hacker News attention.

---

## 5. How to Drastically Cut Costs (Tricks That Actually Work)

1. **Land prompt caching first (see `scaling.md` §3.1, ~1 week of work).** Cuts every subsequent benchmark cost by ~60%. Do this *before* spending a dollar on benchmarks.
2. **Use cheap reasoning models for self-debugging.** DeepSeek-V3 / R1 at $0.27/M input is ~10× cheaper than Sonnet with broadly competitive coding ability. Run wide on DeepSeek, narrow on Sonnet to verify the gap.
3. **Local models for free runs.** Qwen 2.5 Coder 32B via Ollama on a single 24 GB GPU (e.g. RTX 3090/4090) gives you unlimited free benchmark runs at GPT-4-class coding ability. Your machine doesn't need to be GPU-equipped — rent a `vast.ai` 4090 at $0.30/hr; a full SWE-Bench-Lite run is ~$5 of GPU time.
4. **Sample, don't saturate.** A 50-task random subset of SWE-Bench Verified, run 3 times for variance, gives you 90% of the signal at 30% of the cost. Statisticians call this "stratified sampling" and it's how every paper that says "we evaluated on 500 tasks" actually evaluated.
5. **Cache between models.** For each task you can cache the *initial repo state* (Docker layer) and the *task description* once. SWE-Bench-Lite's full state across all 300 tasks is ~60 GB; keep it on disk.
6. **Run only failing tasks on the expensive model.** Cascade: DeepSeek first → for the ~40% it fails, retry on Sonnet. Total Sonnet cost drops to ~$100 for a "full SWE-Bench-Verified result" instead of $270.
7. **Use OpenRouter for trial credits.** $5–10 in free credit when you sign up, plus their "prompt training" credits if you opt-in. Real budget extender.
8. **Skip Terminal-Bench's hardest tier.** The "build a kernel in 4 hours" tasks each cost $5–15 alone in API + are noise-dominated. Stick to the standard tier.
9. **GitHub Actions for compute, not API.** Free 2,000 min/month of CI is enough to run SWE-Bench-Lite once. Don't pay for cloud VMs unless you exceed it.

With these stacked: a full **SWE-Bench-Lite + Aider Polyglot + τ-bench + Terminal-Bench-subset** run across DeepSeek-V3 (primary) + local Qwen Coder (free) + Sonnet (verifier on failures) costs **~$120 instead of $1,500**.

---

## 6. The Recommended Sequence (for a solo maintainer)

1. **Don't benchmark yet.** First land prompt caching (1 week), then headless JSON output (3 days), then sandboxing (3–5 weeks). Without these, the benchmark numbers will be artificially bad (no caching = noisy, no sandbox = can't safely run untrusted SWE-Bench tasks, no JSON = brittle harness).
2. **Then do Tier 1 ($100, 1 week).** Publish numbers in the README. This is enough to be credible on Hacker News.
3. **Then ship features for 2 months.** Re-run Tier 1 each month — track your own progress. This is the actual value of benchmarking: regression detection.
4. **Only do Tier 2 ($500) right before a 2.0 announcement.** Compare against Claude Code 2.x, Codex CLI, OpenCode. Use the comparison for launch marketing.
5. **Never do Tier 3** unless someone is paying you to (a sponsor, an employer, or a grant).

The trap to avoid: running expensive benchmarks before the agent is good enough to score well on them. You'll spend $2,000 to learn "we're at 22% on SWE-Bench" — which is fine — but you could have learned the same thing for $40 with a 50-task subset, and you'd still have $1,960 to spend on the actual improvements.

---

## 7. Concrete First-Run Recipe (copy-paste ready)

**Week 1 deliverable, ~$30 total cost.**

```bash
# 1. Aider polyglot — clone, no Docker needed
git clone https://github.com/Aider-AI/polyglot-benchmark
# Wire forge-osh's --print mode to its runner. Run on 3 models:
#   - DeepSeek-V3   (~$3)
#   - Qwen2.5-Coder via local Ollama (~$0)
#   - Claude Sonnet 4.6 (~$25)
# Outputs: pass@1 per language, total ~225 tasks

# 2. SWE-Bench-Lite, 50-task random sample
git clone https://github.com/princeton-nlp/SWE-bench
# Use their official harness in Docker. Pass forge-osh predictions as JSON.
# 50 random tasks × DeepSeek-V3 ≈ $2

# 3. τ-bench retail
git clone https://github.com/sierra-research/tau-bench
# Tiny — just a tool-use accuracy test. ~$5 across 3 models.

# Total spend: ~$30. Results into bench/results.md, table in README.
```

What goes in the README after this:

| Benchmark | Tasks | Sonnet 4.6 | DeepSeek-V3 | Qwen2.5-Coder (local) |
|---|---|---|---|---|
| Aider Polyglot | 225 | XX% | XX% | XX% |
| SWE-Bench-Lite (sample) | 50 | XX% | XX% | XX% |
| τ-bench retail | 115 | XX% | XX% | XX% |

Honestly label "(subset)" where applicable. Subsets are normal — what's not normal is hiding the methodology.

---

## 8. What This Buys You Long-Term

- **Regression detection**: every release runs Tier 1 in CI; a 5% drop on Aider Polyglot is a blocker before merge.
- **Comparison fodder**: when Claude Code releases v2.1, re-run; show forge-osh tracking or beating it.
- **Honest marketing**: numbers in a table beat any claim in a feature list. Hacker News readers respect this.
- **Cheap forever** if you stick to Tier 1 plus the cascade pattern in §5.6.

The summary: **production-grade benchmarking is affordable if you (a) cache prompts first, (b) sample instead of saturate, and (c) cascade cheap → expensive models.** Skip those tricks and you'll spend $3,000+. Use them and you can sustain a real benchmark suite on ~$30/month.
