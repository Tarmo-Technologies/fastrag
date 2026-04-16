---
name: fastrag-for-VAMS integration — design
description: Close the VAMS-facing HTTP API gaps (/cve, /cwe, /cwe/relation, /ready, /admin/reload) and add airgap packaging (DVD-sized Docker image + multi-corpus bundle format with atomic Arc-swap reload).
type: design
---

# fastrag-for-VAMS integration — design

**Status:** proposed
**Date:** 2026-04-16

## Context

VAMS needs to consume fastrag in airgapped/classified environments. The current fastrag HTTP surface assumes a connected fabric and an operator who can write `/query` filter expressions; VAMS needs structured lookups, a readiness probe, a hot-swap path for sneakernet bundles, and a packaging story that travels on a single DVD.

Much of what VAMS needs is already implemented inside fastrag — CWE taxonomy with descendants traversal, NVD-parsed CVE records with KEV tagging, CWE-rewrite filter, hybrid retrieval, Python client. The remaining work is exposure: taxonomy ancestors, direct ID lookups, bundle reload, readiness distinction, and a one-DVD deployment.

This spec closes those gaps in a single landing.

## Goals

- Expose CVE and CWE records via direct-lookup HTTP endpoints (`GET /cve/{id}`, `GET /cwe/{id}`).
- Expose CWE hierarchy (ancestors + descendants) via `GET /cwe/relation` and via in-document metadata denormalization.
- Add `GET /ready` distinct from `/health`.
- Add `POST /admin/reload` for atomic sneakernet bundle swap.
- Define a multi-corpus bundle format (`cve` + `cwe` + `kev` + taxonomy + manifest).
- Add `Taxonomy::ancestors()` to `fastrag-cwe`.
- Extend the Python client with `.similar()`, `.get_cve()`, `.get_cwe()`, `.cwe_relation()`, `.ready()`, `.reload_bundle()`.
- Ship a DVD-sized Docker image (≤4.7 GB total payload) with embedder and reranker models baked in.

## Non-goals

See § Non-goals section at end — signing in fastrag (Rust-side verification) is deferred to #66; VAMS-side integration code is a separate project; EPSS, multi-tenant, per-corpus reload, streaming reload, async client, horizontal scaling all deferred.

## Architecture

### Bundle shape

```
fastrag-bundle-YYYYMMDD/
├── bundle.json              # top-level manifest (schema_version: 1)
├── corpora/
│   ├── cve/{manifest.json, index.bin, entries.bin}
│   ├── cwe/{manifest.json, index.bin, entries.bin}
│   └── kev/{manifest.json, index.bin, entries.bin}
├── taxonomy/cwe-taxonomy.json   # compiled Research View 1000 (schema_version: 2)
└── bundle.sig                   # optional; VAMS verifies via tarmo-vuln-core before calling /admin/reload
```

`bundle.json`:
```json
{
  "schema_version": 1,
  "bundle_id": "fastrag-20260416",
  "built_at": "2026-04-16T18:00:00Z",
  "corpora": ["cve", "cwe", "kev"],
  "taxonomy": "cwe-taxonomy.json",
  "sources": {
    "cve": {"type": "nvd", "feed_date": "2026-04-15"},
    "kev": {"type": "cisa-kev", "snapshot_date": "2026-04-15"},
    "cwe": {"type": "mitre-cwe", "version": "4.15"}
  }
}
```

Corpus names `cve`, `cwe`, `kev` are hardcoded. Lookup endpoints query the matching corpus by convention — no configuration knob. The bundle-build tool enforces this.

### Runtime layout

```
/var/lib/fastrag/
├── active          → symlink (optional, for operator convenience)
└── bundles/
    ├── fastrag-20260416/    # currently loaded
    └── fastrag-20260417/    # next, staged by VAMS before reload
```

Retention: last N bundles retained on disk, configurable via `--bundle-retention=N` (default 3). Supports instant rollback by re-issuing `/admin/reload` against a prior dir.

### Hot-swap primitive

```rust
pub struct BundleState {
    pub corpora: HashMap<String, Arc<Corpus>>,  // cve, cwe, kev
    pub taxonomy: Arc<CweTaxonomy>,
    pub manifest: BundleManifest,
}

pub struct AppState {
    bundle: ArcSwap<BundleState>,
    embedder: Arc<dyn Embedder>,
    reranker: Option<Arc<dyn Reranker>>,
    admin_token: Option<String>,
    bundles_dir: PathBuf,
}
```

Query handlers call `state.bundle.load_full()` once at entry. The returned `Arc<BundleState>` lives for the full request, so in-flight queries never observe a swap mid-response. `store()` is atomic. Peak memory during reload is ~2× the bundle's resident size; typical bundle is <1 GB resident so peak is ~1.6 GB. A `Mutex<()>` guards the reload handler itself so two concurrent reloads can't race (second caller gets 409 Conflict).

### Module boundaries

- `crates/fastrag/src/bundle.rs` — new: bundle load, validate, atomic swap primitive.
- `crates/fastrag-cwe/src/taxonomy.rs` — extended: `ancestors()`, `parents()`, `ancestors_bounded()`.
- `crates/fastrag-cwe/src/compile.rs` — extended: emit parents map into compiled JSON (schema v2).
- `fastrag-cli/src/http.rs` — new routes + handlers.
- `fastrag-cli/src/args.rs` — new CLI flags (`--bundle-path`, `--bundles-dir`, `--admin-token`, `--bundle-retention`).
- `clients/python/src/fastrag_client/client.py` — new methods.
- `docker/` — new directory with Dockerfile, entrypoint, Makefile targets.
- No change to `Corpus`, `Parser`, ingest, embedder, reranker, query, similar, temporal, CWE rewrite.

## HTTP API additions

All routes gated behind the `retrieval` feature. Auth: existing `X-Fastrag-Token` or `Authorization: Bearer` for reads; `/admin/*` additionally requires a separate `--admin-token` value.

### `GET /cve/{id}`

- Path: `id` — e.g. `CVE-2021-44228`
- Queries corpus `"cve"` via filter `cve_id = {id}`
- 200: `{"id": "...", "text": "...", "metadata": {...}, "score": 1.0}`
- 404: `{"error": "cve_not_found", "id": "..."}`
- 503: corpus not loaded
- Rejects query-string `q`, `top_k`, `filter` — structured lookup, not search.

### `GET /cwe/{id}`

- Path: `id` accepts `89` or `CWE-89`
- Queries corpus `"cwe"` via filter `cwe_id = {id}`
- Response same shape as `/cve/{id}`
- `metadata.parents` and `metadata.children` are populated at bundle-build time from the taxonomy — direct ancestors/descendants available in a single response.

### `GET /cwe/relation`

- Query params: `cwe_id` (required int), `direction` ∈ {`ancestors`, `descendants`, `both`} (default `both`), `max_depth` (optional int)
- Backed by in-memory `Taxonomy::ancestors()` + `Taxonomy::expand()`; no corpus hit.
- Response: `{"cwe_id": 89, "ancestors": [943, 707, 74, 20], "descendants": [564]}` (exact values depend on taxonomy version)
- 400 on malformed `cwe_id`; 404 if unknown.

### `GET /ready`

- Unprotected (like `/health`).
- 200 when: `BundleState` loaded AND embedder reachable AND reranker reachable if configured.
- 503 with `{"ready": false, "reasons": [...]}` otherwise.
- Reason codes: `bundle_not_loaded`, `corpus_{name}_missing`, `embedder_unreachable`, `reranker_unreachable`.

### `POST /admin/reload`

- Admin token required.
- Body: `{"bundle_path": "fastrag-20260417"}` — relative to `--bundles-dir`.
- Flow: resolve + canonicalize path → reject path escape → load bundle + taxonomy → validate (schema versions, required corpora present, manifest consistency) → build new `BundleState` → atomic swap.
- 200: `{"reloaded": true, "bundle_id": "fastrag-20260417", "previous_bundle_id": "fastrag-20260416"}`
- 400 on validation failure; error body identifies the failing check. Prior bundle remains active.
- 409 on concurrent reload.
- 500 on I/O error; prior bundle remains active.

### Unified error body

```json
{"error": "<code>", "message": "<human>", "details": {...}}
```

Follows the existing `/query` error conventions. Codes include `bundle_schema_mismatch`, `corpus_missing`, `path_escape`, `reload_in_progress`, `cve_not_found`, `cwe_not_found`, `unauthorized`, `bundle_not_loaded`.

## CWE taxonomy additions

Current `Taxonomy` stores only descendants. Ancestors require a new map populated at compile time.

```rust
pub struct Taxonomy {
    view: String,
    version: String,
    descendants: HashMap<u32, Vec<u32>>,   // existing
    parents: HashMap<u32, Vec<u32>>,        // new: direct parent edges
}

impl Taxonomy {
    pub fn expand(&self, cwe: u32) -> Vec<u32>;           // existing
    pub fn parents(&self, cwe: u32) -> &[u32];            // new
    pub fn ancestors(&self, cwe: u32) -> Vec<u32>;        // new (BFS over parents)
    pub fn ancestors_bounded(&self, cwe: u32, max_depth: usize) -> Vec<u32>;  // new
}
```

CWE Research View 1000 is a DAG — a node can have multiple parents (e.g. CWE-89 sits under both CWE-943 and CWE-74/707 paths). `ancestors()` BFS dedupes via visited set, returns BFS order. `compile-taxonomy` detects cycles at build time and fails there rather than at runtime.

Schema version on compiled `cwe-taxonomy.json` bumps from 1 → 2. v1 is rejected at bundle load with an error pointing at the rebuild command. No backward-compat reader — bundles are always rebuilt against the runtime version in airgap deployments.

At bundle-build time, each CWE document's `metadata.parents` and `metadata.children` get populated from the taxonomy. `/cwe/{id}` returns hierarchy context in one response; `/cwe/relation` hits the in-memory taxonomy for deep traversal.

## Admin/reload lifecycle

**Handler flow:**

1. Validate admin token (401 on mismatch).
2. Parse body → `bundle_path` (400 on missing/malformed).
3. Resolve path: `bundles_dir.join(bundle_path)`, canonicalize, reject anything that doesn't have `bundles_dir` as prefix (400 `path_escape`).
4. Acquire reload mutex (409 if held).
5. Load + validate: read `bundle.json`, load each corpus, load taxonomy. Validate schema versions, required corpora, manifest consistency. Any failure → 400 with specific error code; prior bundle untouched.
6. Build new `BundleState`, wrap in `Arc`, call `bundle.store(new_state)`.
7. Release mutex. Return 200.

**Startup:** on `serve-http` boot, `--bundle-path <dir>` or `--bundles-dir <dir>/<default>` loads the initial bundle via the same code path. Boot fails hard if initial bundle invalid — service refuses to start without data. `/ready` stays 503 until load completes.

**Path safety:** canonicalized bundle path must have `bundles_dir` as prefix. Symlink escapes rejected.

**Metrics:**
- `fastrag_bundle_reloads_total{result="ok|error"}` counter
- `fastrag_bundle_load_seconds` histogram
- `fastrag_bundle_active_id` gauge labeled with `bundle_id`

## Python client additions

`clients/python/src/fastrag_client/client.py`:

```python
class FastragClient:
    def similar(self, text, threshold, *, max_results=10, corpus=None,
                corpora=None, filter=None, fields=None, verify=None) -> list[SimilarHit]: ...

    def get_cve(self, cve_id: str) -> CveRecord | None: ...
    def get_cwe(self, cwe_id: int | str) -> CweRecord | None: ...
    def cwe_relation(self, cwe_id, *, direction="both", max_depth=None) -> CweRelation: ...
    def ready(self) -> ReadyStatus: ...
    def reload_bundle(self, bundle_path: str, *, admin_token: str | None = None) -> ReloadResult: ...
```

All response types are Pydantic v2 models: `CveRecord`, `CweRecord` (with `cwe_id: int` alongside `id: str`), `CweRelation`, `ReadyStatus`, `ReloadResult`, `SimilarHit`.

Lookup methods return `None` on 404 — idiomatic Python for "structured not-found". `ready()` returns `ReadyStatus(ok=False, reasons=[...])` on 503 rather than raising (probe output, not an error). `reload_bundle()` raises `FastragError` on any non-200.

Constructor adds optional `admin_token` kwarg. Sync only — async client deferred.

## Docker + DVD packaging

**Image:** `debian:12-slim` base. Contains fastrag binary, llama-server, Qwen3-Embedding-0.6B Q8_0 GGUF, BGE-reranker-base Q8_0 GGUF, entrypoint script, `tini` as PID 1.

**Size budget:**
- fastrag binary (stripped): ~70 MB
- llama-server: ~100 MB
- embedder GGUF: ~650 MB
- reranker GGUF: ~400 MB
- base + tini + deps: ~100 MB
- **Image total: ~1.3 GB**

**Entrypoint:**
1. Launch embedder (`llama-server --embedding` on 9001).
2. Launch reranker (`llama-server --rerank` on 9002).
3. Poll both `/health` up to 60s.
4. `exec fastrag serve-http --bundles-dir /var/lib/fastrag/bundles --bundle-path $BUNDLE_NAME --embedder-url http://127.0.0.1:9001 --reranker-url http://127.0.0.1:9002 --admin-token $FASTRAG_ADMIN_TOKEN`

`tini` handles signal forwarding and zombie reaping.

**DVD ISO target (`make dvd-iso`):**

```
fastrag-airgap.iso (≤4.7 GB single-layer DVD)
├── README.md                           # install doc
├── image/
│   ├── fastrag-X.Y.Z.tar.gz            # ~1.3 GB docker save
│   └── SHA256SUMS
└── bundles/
    └── fastrag-sample/                 # ~1.5-2 GB sample bundle
        ├── bundle.json
        ├── corpora/{cve,cwe,kev}/
        └── taxonomy/cwe-taxonomy.json
```

**Operator flow:**
```
mount /dev/sr0 /mnt/dvd
docker load < /mnt/dvd/image/fastrag-X.Y.Z.tar.gz
cp -r /mnt/dvd/bundles/fastrag-sample /var/lib/fastrag/bundles/
docker run -d --name fastrag -p 8080:8080 \
    -v /var/lib/fastrag/bundles:/var/lib/fastrag/bundles \
    -e BUNDLE_NAME=fastrag-sample \
    -e FASTRAG_READ_TOKEN=... \
    -e FASTRAG_ADMIN_TOKEN=... \
    fastrag:X.Y.Z
curl http://localhost:8080/ready
```

**CI size gates:**
- image tarball ≤ 1.5 GB
- ISO ≤ 4.4 GB (300 MB safety margin below DVD ceiling)

**Phone-home audit (CI gate):**
- Container boots with `--network=none` and reaches `/ready` (503 is acceptable — 500 or crash fails).
- `strace` audit during boot: no outbound DNS, no TCP connect to external addresses. Loopback only.

## Data model

No change to `Corpus`, `SearchHitDto`, or embedding schema.

New serializable types:
- `BundleManifest` (matches `bundle.json`)
- `BundleState` (runtime only, not persisted)
- `CweTaxonomy` schema v2 (descendants + parents)

## Verification

### Unit tests

- `fastrag-cwe`: `ancestors`, `ancestors_bounded`, multi-parent DAG, unknown CWE returns empty.
- `fastrag/src/bundle.rs`: manifest validation — missing corpus, bad schema, malformed JSON.
- HTTP handlers: each endpoint — happy path, 400, 404, auth rejection.

### Integration tests (`tests/`)

- `bundle_load.rs` — load fixture bundle, assert structure.
- `admin_reload_e2e.rs` — start server, reload, verify swap.
- `admin_reload_concurrent.rs` — concurrent reload returns 409.
- `admin_reload_path_escape.rs` — traversal rejected.
- `admin_reload_rollback.rs` — reload A → B → A works.
- `cve_lookup_e2e.rs`, `cwe_lookup_e2e.rs`, `cwe_relation_e2e.rs` — full HTTP flow.
- `ready_probe_e2e.rs` — 503 before load, 200 after.

Fixture bundles in `tests/fixtures/bundles/`:
- `minimal/` — 10 CVEs, 20 CWEs, 5 KEV entries
- `corrupted-missing-cve/` — asserts validation failure
- `corrupted-bad-taxonomy/` — asserts schema check

### Python client tests

- One test file per new method, covering happy path + 4xx + 5xx via `responses`.
- Type check: `mypy --strict clients/python/`.
- Round-trip: `get_cwe(89).metadata['parents']` matches `cwe_relation(89).ancestors[:N]`.

### Docker CI checks

- `docker-build-size.sh` — fails image > 1.5 GB.
- `docker-iso-size.sh` — fails ISO > 4.4 GB.
- `docker-no-phone-home.sh` — `--network=none` boot + `strace` audit.
- `docker-smoke.sh` — boot with sample bundle, hit every endpoint.

### Regression

Existing `/query`, `/batch-query`, `/ingest`, `/similar` test suites run unchanged against the new `BundleState`-backed server. Matrix eval uses fixture bundle, not production; `docs/eval-baselines/current.json` recapture not required by this plan.

## Rollout

Commits land in this order (one landing, stacked):

1. **Taxonomy ancestors** — `fastrag-cwe` gets `ancestors()` + `parents()` + schema v2. `compile-taxonomy` emits v2 JSON. Old bundles rejected.
2. **Bundle module** — `crates/fastrag/src/bundle.rs` with `BundleState`, `ArcSwap`, load + validate. No wire-up yet.
3. **AppState + startup** — thread `BundleState` through `serve-http` init; `--bundle-path`, `--bundles-dir`, `--admin-token`, `--bundle-retention` CLI flags.
4. **GET /cve, GET /cwe, GET /cwe/relation** — lookup endpoints backed by `BundleState`.
5. **GET /ready** — readiness probe distinct from `/health`.
6. **POST /admin/reload** — atomic reload handler + concurrent guard + metrics.
7. **Python client** — `.similar()`, `.get_cve()`, `.get_cwe()`, `.cwe_relation()`, `.ready()`, `.reload_bundle()`.
8. **Docker image + Makefile target** — Dockerfile, entrypoint, CI size gates.
9. **DVD ISO target** — `make dvd-iso`, operator docs at `docs/airgap-install.md`.

Each commit keeps the tree shippable. Commits 1-2 are dead code until commit 3 wires them up.

## Risks

**R1. Memory spike during reload.** Peak ~2× resident bundle size during swap. Airgap VMs must be sized with 4+ GB RAM headroom. Documented in install docs.

**R2. DVD size pressure as CVE corpus grows.** Current estimate leaves ~1 GB headroom. If NVD doubles, options: drop reranker to Q6_K (saves ~100 MB), move to dual-layer DVD (8.5 GB), or ship delta bundles (deferred). CI size gate surfaces the threshold before it's breached.

**R3. Schema v2 taxonomy breaks old bundles.** Intentional — airgap deployments rebuild bundles against the runtime fastrag. Loader error points at the rebuild command.

**R4. No bundle signature verification in fastrag.** Relies on VAMS-side Python verification via `tarmo-vuln-core`. Deferred to #66. Accepted for v1; re-evaluated when a separation-of-duties customer requires it.

**R5. Admin token rotation requires restart.** `--admin-token` is set at launch. Acceptable for airgap where restarts are planned operations.

## Non-goals (deferred or out of scope)

Deferred to follow-up issues:
- Bundle signature verification in fastrag (Rust-side) — #66.
- Streaming reload progress.
- Bundle upload via HTTP POST.
- Per-corpus reload.
- EPSS scores.
- Async Python client.
- Horizontal scaling.

Out of scope entirely:
- Signing in fastrag — stays in `tarmo-vuln-core`.
- VAMS-side integration code — separate project.
- Multi-tenant corpus isolation.
- CVE auto-enrichment on query (contradicts airgap).
- Bundle diff / incremental update.

## References

- `#66` — airgap hardening: independent corpus signature verification in fastrag (deferred).
- `crates/fastrag-cwe/src/taxonomy.rs` — existing descendants API that `ancestors()` mirrors.
- `fastrag-cli/src/http.rs:1563-1742` — existing `/similar` endpoint; pattern for new handlers.
- `crates/fastrag/src/filter/cwe_rewrite.rs` — existing CWE-expansion filter; `cwe_id` field convention.
- `crates/fastrag-nvd/src/metadata.rs` — existing `cve_id`, `kev_flag` metadata schema consumed by `/cve/{id}`.
- `tarmo_vuln_core.signing.ReportSigner` — Option A signature verification (called by VAMS before `/admin/reload`).
- `docs/superpowers/specs/2026-04-11-security-corpus-hygiene-design.md` — existing KEV tagging; consumed by the `kev` bundle corpus.
