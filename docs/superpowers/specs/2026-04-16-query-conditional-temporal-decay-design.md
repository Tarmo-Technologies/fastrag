---
name: Query-conditional temporal decay — v1
description: First-class per-query temporal policy API, abstaining regex detector, late-stage decay injection. Replaces fixed-halflife TemporalOn (proven to regress historical queries by 15pp hit@5).
type: design
---

# Query-conditional temporal decay — v1

**Status:** proposed
**Date:** 2026-04-16
**Closes:** #53 (partially — anchored-past intent, compare/multi-period, doc volatility, timestamp hierarchy deferred to follow-up issues)

## Context

The 2026-04-15 gold-set expansion produced per-bucket eval signal showing the existing fixed-halflife decay (`TemporalOn` variant, 730-day halflife) regresses hit@5 by 15.4pp on historical queries, 4.7pp on neutral queries, and only marginally helps recency_seeking (−2.6pp vs Primary — still net negative because the halflife isn't aggressive enough to overcome corpus lexical bias).

A single global halflife cannot satisfy all three intent classes at once. The question is how to route.

A critical external review flagged several design flaws in an earlier three-mode proposal: CVE-year heuristics misclassify topical identifiers as historical intent, neutral→mild decay is wrong by the eval data, the design ignores anchored-past and compare/multi-period intent classes, decay-before-rerank can suppress correct old documents before the reranker sees them, and a global `--time-decay-mode` CLI flag is a footgun for production use.

This v1 takes the conservative subset: ship a per-query API, a high-precision abstaining regex detector, late-stage decay injection, and conservative parameters. Defer anchored-past, multi-period, doc volatility, and timestamp hierarchy to follow-up issues where they can be designed in isolation.

## Goals

- Per-query `TemporalPolicy` API surface, consumable by VAMS (scan-date-aware correlation) and pentest tooling (explicit freshness requests for PoC hunting).
- Abstaining regex detector: high-precision recency recognition, default to `Off` when no positive signal. No CVE-year heuristics. No historical class in v1.
- Late-stage decay: apply temporal factor to the final post-rerank score, not during RRF fusion. Correct old docs survive to rerank regardless of decay.
- Conservative defaults that do not regress any bucket when detector abstains.
- Eval variants `TemporalAuto` (regex) and `TemporalOracle` (gold-set axes) + route-regret metric (`mrr_oracle − mrr_auto`).
- Retire `ConfigVariant::TemporalOn` — empirically broken, carries no diagnostic value forward.

## Non-goals (deferred to follow-up issues)

- Anchored-past intent (`as of June 2023` — decay relative to the anchor, not now). Requires reference-time resolution and a new policy variant. Separate issue.
- Compare/multi-period retrieval. Needs coverage-aware metrics (temporal precision/coverage), not a decay knob. Separate issue.
- Document-volatility multipliers (canonical explanations decay slower than PoCs). Requires doc-type taxonomy on the corpus. Separate issue.
- Timestamp field hierarchy (`last_modified` → `published_date` → source fallback). The coalesce work in `df4c7cc` handles ordered-field selection but does not implement per-source fallbacks. Separate issue.
- Dateless-doc per-route policy. Existing `dateless_prior` knob stays; per-route overrides deferred.
- Learned intent classifier (TF-IDF + char n-grams). Needs routed-query logs for training. Separate issue once logging lands.
- Global `--time-decay-mode` CLI flag as production default. Kept for eval and ablation only, not recommended in user-facing docs.

## Architecture

Three layered changes. Each is independently reviewable.

### Layer 1 — `TemporalPolicy` API

**New:** `crates/fastrag/src/corpus/temporal.rs`

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum TemporalPolicy {
    #[default]
    Auto,              // use configured detector; abstains to Off when no signal
    Off,               // no decay, regardless of query
    FavorRecent(Strength),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength { Light, Medium, Strong }

impl Strength {
    pub fn halflife(self) -> Duration {
        match self {
            Strength::Light  => Duration::from_secs(365 * 86_400), // 1y
            Strength::Medium => Duration::from_secs(180 * 86_400), // 6mo
            Strength::Strong => Duration::from_secs(60  * 86_400), // 2mo
        }
    }
    pub fn weight_floor(self) -> f32 {
        match self {
            Strength::Light  => 0.75,
            Strength::Medium => 0.60,
            Strength::Strong => 0.45,
        }
    }
}
```

Parameter choices: `Light` defaults preserve 1-year-old docs at ~0.87 weight (minimal damage). `Medium` is the default when auto detects recency — 6-month halflife with 0.60 floor keeps a 2-year-old canonical explanation viable if it outranks a fresh post semantically. `Strong` is opt-in per query for hunt-for-freshest-PoC scenarios. No variant in v1 uses a halflife below 60 days; the 30d/weight-0.3 from the original draft is dropped as empirically too destructive.

### Layer 2 — Abstaining detector

```rust
pub trait TemporalDetector: Send + Sync {
    fn detect(&self, query: &str) -> TemporalPolicy;
}

pub struct AbstainingRegexDetector { /* compiled regex patterns */ }
pub struct OracleDetector { /* intent label passed per-query */ }
```

`AbstainingRegexDetector::detect` matches only high-precision recency markers:

- `\b(latest|newest|current(ly)?|newer)\b`
- `\brecent(ly)?\b` when followed by noun forms (`advisory|exploit|bypass|CVE|disclosure|vulnerability|patch|guidance`) — not bare `recently`
- `\bstill (exploited|in KEV|vulnerable|unpatched)\b`
- `\bas of (today|now|this (week|month))\b`
- `\b(this week|this month)\b` (deliberately excludes `this year` — too broad for 6-month default halflife)
- Explicit current-year marker `\b2026\b` only when paired with `(CVE|vulnerability|advisory|disclosure|exploit|PoC|mitigation|patch)` within 5 tokens. Bare `2026` is ambiguous (could be a CVE year, version string, port) — abstain.

**No CVE-year historical heuristic.** CVE identifiers with past years (`CVE-2014-0160`) are topical, not historical intent. The detector returns `TemporalPolicy::Off` for them — same as for any query without a positive recency signal.

**No historical class in v1.** `as of 2021`, `in 2014`, `back in` phrasings all return `Off` (no decay). This is a deliberate simplification: historical anchoring needs `reference_time` support, which is deferred. Returning `Off` for historical-looking queries is correct behavior even without the anchor concept — we just don't apply *any* decay.

`OracleDetector` is eval-only: takes `Option<TemporalIntent>` at construct time, maps `RecencySeeking → FavorRecent { Medium }`, everything else → `Off`.

### Layer 3 — Late-stage injection

Current flow: `query_hybrid` does BM25 + dense → RRF → `apply_decay` → return. Reranker runs after on the decayed candidates.

New flow: `query_hybrid` drops its internal decay call. The corpus orchestrator (`corpus/mod.rs::query`) runs the retrieval pipeline (hybrid or dense) → rerank → **then** resolves the effective `TemporalPolicy` for this query and applies decay to the post-rerank score.

This means:

1. The reranker always sees the un-decayed candidate set. An old canonical doc that would have been decay-suppressed below the rerank-over-fetch cutoff survives.
2. Decay becomes a final-score modifier, not a candidate-selection filter.
3. The `apply_decay` helper stays pure-function — only the call site changes.

Implementation: a new `apply_temporal_policy(results: &[ScoredHit], policy: &TemporalPolicy, query: &str, detector: &dyn TemporalDetector, dates: &[Option<NaiveDate>], now: DateTime<Utc>) -> Vec<ScoredHit>` wrapper in `corpus/temporal.rs` resolves `Auto → detector.detect(query)`, builds a per-query `TemporalOpts` from the policy's strength params, and calls the existing `apply_decay`. For `Off`, short-circuits and returns the input slice unchanged.

### Wire-up

**Rust API (`corpus/mod.rs::QueryOpts`):** new field `temporal_policy: TemporalPolicy` (defaults to `Auto`). The orchestrator builds the detector once per corpus and reuses it.

**HTTP API (`/query` body):** new optional `"temporal_policy"` field — one of `"auto"`, `"off"`, `{"favor_recent": "light|medium|strong"}`. Absent = `auto`.

**CLI:** new flag on `query` and `serve-http` subcommands: `--temporal-policy <auto|off|favor-recent-light|favor-recent-medium|favor-recent-strong>` (default `auto`). Existing `--time-decay-field` still required to enable the pipeline at all (unchanged semantics — no dated corpus means no policy can apply).

**Deprecated (kept with warning):** `--time-decay-halflife`, `--time-decay-weight`, `--time-decay-blend`, `--time-decay-dateless-prior`. These continue to work only when `--temporal-policy` is explicitly set to a non-`auto` value, and emit a one-line stderr deprecation notice. Schedule removal after the parameter-sweep follow-up lands.

### Matrix eval changes

**Retired:** `ConfigVariant::TemporalOn` (fixed 730d halflife).

**New:** `ConfigVariant::TemporalAuto` — runs the shipped `AbstainingRegexDetector` over gold-set queries. Uses `Medium` strength when detector fires.

**New:** `ConfigVariant::TemporalOracle` — runs `OracleDetector` wired to `entry.axes.temporal_intent`. Upper bound on what the regex could achieve with perfect routing.

`ConfigVariant::all()` stays at 5 variants (Primary, NoRerank, NoContextual, DenseOnly, TemporalAuto) in the canonical list. TemporalOracle is added but marked eval-internal and not run by default. A new `--variants` flag on `fastrag eval` opts into oracle runs when diagnosing regex quality.

**New metric in `VariantReport`:** `route_regret: Option<f64>` — only populated for TemporalOracle, computed as `oracle.mrr_at_10 - auto.mrr_at_10` at report-build time if both variants ran. Populated into `MatrixReport.summary` rather than per-variant to keep schema clean.

## Data model

No change to baseline schema v2 (already supports variable variant sets and per-bucket metrics). `TemporalPolicy` is a new crate-internal type, not persisted. Regex patterns live as static `OnceLock<Regex>` inside the detector.

## Verification

### Unit (decay math unchanged)

- `apply_decay` tests keep passing without modification (pure function, no call-site knowledge).

### Unit (new)

- `AbstainingRegexDetector` — for each pattern family (latest, newer, still X, as-of-today, explicit 2026+noun): expected-positive fixture + expected-negative fixture to guard against false positives. The false-positive set includes at minimum: `current user guide`, `latest attempt to install X`, `recent commit`, `CVE-2026-0001` (bare identifier, no freshness word), `port 2026 scan`.
- `OracleDetector` — round-trip per intent variant.
- `Strength::halflife / weight_floor` — constant-value assertion.
- `apply_temporal_policy` — `Off` returns input unchanged (same length, same order, same scores, same allocation strategy if possible); `FavorRecent` routes to `apply_decay`; `Auto` delegates to detector.

### Integration

- `corpus/mod.rs::query` with `temporal_policy: Auto` + query=`"latest Log4j advisory"` + corpus containing a 2021 and 2026 writeup: 2026 doc ranks above 2021 doc.
- Same setup with query=`"describe CVE-2021-44228"`: ranking matches Primary (no-decay) ordering. This is the "abstain → no regression" contract.
- Same setup with query=`"as of 2014 how did Shellshock work"`: ranking matches Primary. No historical class = no decay = same as Primary.
- Late-injection contract: a synthetic corpus where the top-lexical old doc would be decay-suppressed below the rerank over-fetch cutoff under the old pre-rerank path. With v1 late injection, the old doc reaches rerank, gets semantically promoted, and appears in the final top-k. The test fails if decay is applied pre-rerank.

### Eval gate (baseline recapture required)

Recapture `docs/eval-baselines/current.json` with the new variant set, then enforce in the regression gate:

- `TemporalAuto.buckets.temporal_intent.historical.hit_at_5 ≥ Primary.buckets.temporal_intent.historical.hit_at_5 − 0.02` (abstain contract — must not regress historical by more than 2pp).
- `TemporalAuto.buckets.temporal_intent.neutral.hit_at_5 ≥ Primary.buckets.temporal_intent.neutral.hit_at_5 − 0.01` (abstain contract tighter on the larger bucket).
- `TemporalAuto.buckets.temporal_intent.recency_seeking.mrr_at_10 ≥ Primary.buckets.temporal_intent.recency_seeking.mrr_at_10` (directional improvement — auto must at least not hurt recency queries).
- `TemporalOracle.buckets.temporal_intent.recency_seeking.mrr_at_10 ≥ TemporalAuto.buckets.temporal_intent.recency_seeking.mrr_at_10` (oracle is upper bound on regex).

These gates live alongside the existing per-bucket slack system; violations fail `fastrag eval --baseline`.

## Rollout

One landing, three commits stacked:

1. **Temporal policy module** — `temporal.rs` with types, detector trait, regex detector, oracle detector. All unit tests in-file. No wire-up yet.
2. **Late injection + wire-up** — drop decay from `query_hybrid`, add `apply_temporal_policy` at the orchestrator, thread `TemporalPolicy` through `QueryOpts` / HTTP body / CLI flag. Integration tests.
3. **Eval matrix swap + baseline recapture** — retire `TemporalOn`, add `TemporalAuto` and `TemporalOracle`, add `route_regret` reporting, recapture `current.json`, update gate assertions.

Each commit keeps the tree shippable. Between commits 1 and 2 the new module is dead code; between 2 and 3 the eval uses the retired variant name. Only commit 3 changes the baseline shape — they land together.

## Risks

**R1. Abstaining detector misses recency intent.** Real false negatives in the wild (queries like `still exploited`, `newer bypass`) get `Off` and lose freshness. Mitigation: `route_regret` metric on oracle runs quantifies this gap. If regret is large, expand the regex (cheap) or file the learned-classifier follow-up.

**R2. Late injection changes ranking for existing users.** Anyone running with `--time-decay-field` today gets different results after v1. Mitigation: documented in changelog. The new ranking is more correct (reranker semantics win over temporal heuristics), and the opt-in surface is small.

**R3. Regex false positives damage a neutral query.** Unlikely given the whitelist construction (every pattern requires strong recency vocabulary), but a query like `latest documentation for PyYAML` hitting `FavorRecent { Medium }` would lightly decay a canonical 3-year-old security writeup. Mitigation: the `Medium` halflife+floor combination is tuned conservative (180d halflife, 0.60 floor — 3y doc retains ~0.60 × canonical score, still viable if semantically stronger). The eval gate catches bulk damage.

**R4. Deprecated flags cause friction.** Users scripted against `--time-decay-halflife` see stderr warnings. Mitigation: deprecation warning points at `--temporal-policy`. Removal date set when the parameter-sweep follow-up lands (not this landing).

**R5. Parameter defaults are still untuned.** `Light/Medium/Strong` numbers are eval-informed guesses, not optimized. Mitigation: the parameter-sweep follow-up issue owns this. v1 ships functional behavior; tuning is a separate landing.

## References

- `docs/superpowers/specs/2026-04-15-gold-set-temporal-expansion-design.md` — the eval data that disproves fixed-halflife.
- `docs/superpowers/specs/2026-04-14-temporal-decay-hybrid-retrieval-design.md` — the decay math and infrastructure this reuses.
- Issue #53 — closes partially; defers anchored-past, multi-period, volatility to new issues filed alongside this spec.
- `docs/eval-baselines/current.json` — baseline that v1 recaptures.
- External review feedback (2026-04-16) informing the abstaining detector, late injection, and per-query API decisions.
