"""docker/Dockerfile.airgap model-fetcher: every model is fetched at a pinned
Hugging Face commit and checked against a pinned sha256, and a file whose
digest does not match fails the build.

The behavioural tests run the stage's real RUN script under /bin/sh (as
BuildKit does) with a stub `curl` on PATH, so no image is built and no
network is used.
"""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

import pytest

DOCKERFILE = Path(__file__).resolve().parents[2] / "docker" / "Dockerfile.airgap"

DOWNLOAD = re.compile(
    r'"\$\{base\}/(?P<repo>[^"/]+/[^"/]+)/resolve/(?P<rev>[^"/]+)/(?P<path>[^"]+)"'
    r'\s+-o\s+"(?P<dest>[^"]+)"'
)
SHA_CHECK = re.compile(r'echo "\$\{(?P<var>\w+)\}  (?P<dest>[^"]+)" \| sha256sum -c -')
VAR = re.compile(r"\$\{(\w+)\}")

STUB_CURL = """#!{python}
import json, os, sys
args = sys.argv[1:]
dest = args[args.index("-o") + 1]
url = next(a for a in args if a.startswith("https://"))
header = args[args.index("-H") + 1]
with open(os.environ["STUB_LOG"], "a") as fh:
    fh.write(json.dumps({{"url": url, "dest": dest, "header": header}}) + "\\n")
with open(dest, "wb") as fh:
    fh.write(b"GGUF stub for " + os.path.basename(dest).encode())
"""


def stub_bytes(dest: str) -> bytes:
    return b"GGUF stub for " + Path(dest).name.encode()


def model_fetcher_stage() -> list[str]:
    """The model-fetcher stage's instructions, continuation lines joined."""
    joined = re.sub(r"\\\n", " ", DOCKERFILE.read_text())
    stage: list[str] = []
    inside = False
    for line in joined.splitlines():
        line = line.strip()
        if line.startswith("FROM "):
            inside = line.endswith(" AS model-fetcher")
            continue
        if inside and line and not line.startswith("#"):
            stage.append(line)
    assert stage, "no model-fetcher stage in Dockerfile.airgap"
    return stage


def stage_args() -> dict[str, str]:
    args: dict[str, str] = {}
    for line in model_fetcher_stage():
        if line.startswith("ARG "):
            name, _, default = line[4:].partition("=")
            args[name] = default
    return args


def fetch_script() -> str:
    runs = [line for line in model_fetcher_stage() if "id=hf_token" in line]
    assert len(runs) == 1, runs
    return re.sub(r"^RUN\s+--mount=\S+\s+", "", runs[0])


def expand(text: str, args: dict[str, str]) -> str:
    return VAR.sub(lambda m: args.get(m.group(1), m.group(0)), text)


def pinned_files() -> list[dict[str, str]]:
    """repo, rev, path, dest (ARG defaults expanded) and the sha256 ARG per download."""
    args = stage_args()
    script = fetch_script()
    sha_var = {expand(m["dest"], args): m["var"] for m in SHA_CHECK.finditer(script)}
    return [
        {
            "repo": m["repo"],
            "rev": m["rev"],
            "path": m["path"],
            "dest": m["dest"],
            "sha_var": sha_var.get(m["dest"], ""),
        }
        for m in DOWNLOAD.finditer(expand(script, args))
    ]


def test_every_download_is_pinned_to_a_commit_and_a_sha256() -> None:
    script = fetch_script()
    files = pinned_files()
    # Every curl in the stage is a download the parser saw.
    assert len(files) == script.count("curl ") == 3
    assert "resolve/main" not in DOCKERFILE.read_text()
    args = stage_args()
    for f in files:
        assert re.fullmatch(r"[0-9a-f]{40}", f["rev"]), f
        assert f["sha_var"], f"no sha256sum -c for {f['dest']}"
        assert re.fullmatch(r"[0-9a-f]{64}", args[f["sha_var"]]), f
    assert {f["dest"] for f in files} == {
        "/models/snowflake-arctic-embed-l-Q8_0.GGUF",
        "/models/reranker-modernbert-gooaq-bce-onnx/model.onnx",
        "/models/reranker-modernbert-gooaq-bce-onnx/tokenizer.json",
    }
    # One digest per file, never one value reused for two.
    assert len({args[f["sha_var"]] for f in files}) == 3


def run_fetch(
    tmp_path: Path, overrides: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    """Run the stage's RUN script with /models and the secret under tmp_path."""
    stub_bin = tmp_path / "bin"
    stub_bin.mkdir()
    curl = stub_bin / "curl"
    curl.write_text(STUB_CURL.format(python=sys.executable))
    curl.chmod(0o755)
    token = tmp_path / "hf_token"
    token.write_text("stub-token")
    models = tmp_path / "models"
    (models / stage_args()["RERANK_DIR_NAME"]).mkdir(parents=True)

    script = fetch_script()
    assert "/run/secrets/hf_token" in script
    script = script.replace("/run/secrets/hf_token", str(token))
    # Every absolute /models/ path, quoted or after the digest in a check line.
    script, remapped = re.subn(r'(?<=[\s"])/models/', f"{models}/", script)
    assert remapped >= 3 and " /models/" not in script and '"/models/' not in script
    env = {
        **stage_args(),
        **overrides,
        "PATH": f"{stub_bin}:/usr/bin:/bin",
        "STUB_LOG": str(tmp_path / "curl.log"),
    }
    return subprocess.run(
        ["/bin/sh", "-c", script], env=env, capture_output=True, text=True, timeout=30
    )


def matching_digests(tmp_path: Path) -> dict[str, str]:
    models = tmp_path / "models"
    return {
        f["sha_var"]: hashlib.sha256(
            stub_bytes(f["dest"].replace("/models/", f"{models}/"))
        ).hexdigest()
        for f in pinned_files()
        if f["sha_var"]
    }


def test_fetch_requests_pinned_revisions_and_succeeds_on_matching_digests(
    tmp_path: Path,
) -> None:
    result = run_fetch(tmp_path, matching_digests(tmp_path))
    assert result.returncode == 0, result.stdout + result.stderr
    log = [
        json.loads(line) for line in (tmp_path / "curl.log").read_text().splitlines()
    ]
    assert [entry["url"] for entry in log] == [
        f"https://huggingface.co/{f['repo']}/resolve/{f['rev']}/{f['path']}"
        for f in pinned_files()
    ]
    assert {entry["header"] for entry in log} == {"Authorization: Bearer stub-token"}
    tokenizer = (
        tmp_path / "models" / "reranker-modernbert-gooaq-bce-onnx" / "tokenizer.json"
    )
    assert tokenizer.read_bytes() == b"GGUF stub for tokenizer.json"


@pytest.mark.parametrize("bad", [f["dest"] for f in pinned_files()])
def test_fetch_fails_when_any_file_digest_differs(tmp_path: Path, bad: str) -> None:
    digests = matching_digests(tmp_path)
    bad_var = next(f["sha_var"] for f in pinned_files() if f["dest"] == bad)
    assert bad_var, f"no sha256 pin for {bad}"
    digests[bad_var] = "0" * 64
    result = run_fetch(tmp_path, digests)
    assert result.returncode != 0
    assert f"{Path(bad).name}: FAILED" in result.stdout
    assert "did NOT match" in result.stderr


def test_fetch_fails_on_the_shipped_pins_with_other_bytes(tmp_path: Path) -> None:
    # The stub serves bytes that are not the real models: the pinned digests
    # must reject them even though the GGUF magic check would pass.
    result = run_fetch(tmp_path, {})
    assert result.returncode != 0
    assert "snowflake-arctic-embed-l-Q8_0.GGUF: FAILED" in result.stdout
