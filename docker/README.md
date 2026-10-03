# fastrag airgap image

Debian-12 slim runtime with `fastrag`, `llama-server`, and pre-staged GGUFs
for a completely offline security lookup service. Built by
`docker/Dockerfile.airgap`.

## Layout at runtime

```
/usr/local/bin/fastrag              # fastrag CLI (ENTRYPOINT via tini)
/usr/local/bin/llama-server         # spawned as a subprocess per role
/opt/fastrag/lib/                   # libonnxruntime.so.* (on LD_LIBRARY_PATH)
/opt/fastrag/models/                              # $FASTRAG_MODEL_DIR
    snowflake-arctic-embed-l-Q8_0.GGUF            # llama-cpp embedder (airgap profile)
    reranker-modernbert-gooaq-bce-onnx/           # ONNX reranker (in-process via ort)
        model.onnx
        tokenizer.json
/var/lib/fastrag/bundles/                         # mount your bundles here
```

With no command, the container entrypoint writes a temporary `fastrag.toml`
at startup and selects the `airgap` embedder profile. That profile resolves the
bundled Snowflake Arctic Embed L GGUF through the llama-cpp backend, and
`--rerank onnx` loads the ModernBERT-gooaq-bce reranker directly via `ort`.
Both models are Tarmo-owned HuggingFace re-hosts; see the no-Chinese-origin
compliance note in `feedback_no_chinese_models.md`. The build fetches each
model at a pinned commit and fails if a file's sha256 differs from its pin
(`EMBED_*` and `RERANK_*` build args in `Dockerfile.airgap`).

## Environment variables

| Variable                | Required | Purpose                                                 |
|-------------------------|----------|---------------------------------------------------------|
| `BUNDLE_NAME`           | yes*     | Directory under `/var/lib/fastrag/bundles/` to load.    |
| `FASTRAG_TOKEN`         | no       | Read token for `/query`, `/cve`, `/cwe`, `/kev`, etc.   |
| `FASTRAG_ADMIN_TOKEN`   | no       | Admin token for `/admin/reload`. Must differ from read. |
| `BUNDLES_DIR`           | no       | Override bundles root (default `/var/lib/fastrag/bundles`). |
| `PORT`                  | no       | Listen port inside the container (default `8080`).      |
| `FASTRAG_MODEL_DIR`     | preset   | Points at the pre-staged GGUFs used by the airgap profile. |

\* Only the default command reads `BUNDLE_NAME`, `BUNDLES_DIR` and `PORT`; a
command you pass names its own bundle and port (see below). `fastrag
serve-http` reads the two tokens from the environment itself, and
`FASTRAG_HOST` defaults to `0.0.0.0`, with or without a command.

## Run

```bash
docker run --rm -p 8080:8080 \
    -v /path/to/bundles:/var/lib/fastrag/bundles:ro \
    -e BUNDLE_NAME=fastrag-20260416 \
    -e FASTRAG_TOKEN=<read-token> \
    -e FASTRAG_ADMIN_TOKEN=<admin-token> \
    fastrag:<tag>
```

### Your own command

Arguments after the image name, or a compose `command:`, replace the default
command. A fastrag subcommand or flag runs under `fastrag`, and an executable
on `PATH` runs as given; the entrypoint `exec`s it, so tini's signals reach it.

```yaml
command: [serve-http, --config, /etc/fastrag/fastrag.toml, --embedder-profile, vams,
          --bundle-path, /var/lib/fastrag/bundles/vams-lookup-v1,
          --corpus, vams-findings=/var/lib/fastrag/corpora/vams-findings, --port, "8080"]
```

The image has no network at start-up and runs no Ollama, so the embedder
profile you select must use the bundled GGUF: `backend = "llama-cpp"`,
`model = "/opt/fastrag/models/snowflake-arctic-embed-l-Q8_0.GGUF"`.

## Lookup endpoints

With `FASTRAG_TOKEN` configured, the bundled HTTP API exposes direct lookup
routes for downstream consumers such as VAMS:

- `GET /cve/{cve_id}` → exact CVE record lookup from the `cve` corpus
- `GET /cwe/{cwe_id}` → exact CWE record lookup from the `cwe` corpus
- `GET /cwe/relation?cwe_id=89` → CWE ancestor/descendant traversal
- `GET /kev/{cve_id}` → exact KEV record lookup from the `kev` corpus

All direct lookup routes return `{"hits": [...]}` on success and `404` with a
typed `*_not_found` error when the corpus contains no matching record.

## Build, size, audit, smoke

All three gates are wired into `make`:

```bash
make airgap-image            # docker build
make airgap-size             # fails if image > 1.5 GiB
make airgap-no-phone-home    # boots with --network=none, checks logs
make airgap-smoke            # fixture bundle + profile-first /health /ready /cwe/relation
```
