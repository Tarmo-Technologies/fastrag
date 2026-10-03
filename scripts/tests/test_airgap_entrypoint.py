"""docker/entrypoint.sh: a caller's command (compose `command:`, `docker run
IMAGE ...`) is honoured; with none, the bundle-driven serve-http default runs.

A stub `fastrag` on PATH records the argv, PID and FASTRAG_HOST it was exec'd
with, so no image or Rust build is needed.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

ENTRYPOINT = Path(__file__).resolve().parents[2] / "docker" / "entrypoint.sh"

# VAMS docker-compose.yml `command:` for the fastrag service, with the
# default env substituted (vams-lookup-v1 bundle).
VAMS_COMMAND = [
    "serve-http",
    "--config",
    "/etc/fastrag/fastrag.toml",
    "--embedder-profile",
    "vams",
    "--bundles-dir",
    "/var/lib/fastrag/bundles",
    "--bundle-path",
    "/var/lib/fastrag/bundles/vams-lookup-v1",
    "--corpus",
    "vams-findings=/var/lib/fastrag/corpora/vams-findings",
    "--port",
    "8080",
]

STUB = """#!{python}
import json, os, sys
with open(os.environ["STUB_RECORD"], "w") as fh:
    json.dump(
        {{
            "name": os.path.basename(sys.argv[0]),
            "argv": sys.argv[1:],
            "pid": os.getpid(),
            "fastrag_host": os.environ.get("FASTRAG_HOST"),
        }},
        fh,
    )
"""


@pytest.fixture
def sandbox(tmp_path: Path) -> dict[str, Path]:
    stub_bin = tmp_path / "bin"
    stub_bin.mkdir()
    for name in ("fastrag", "other-tool"):
        stub = stub_bin / name
        stub.write_text(STUB.format(python=sys.executable))
        stub.chmod(0o755)
    bundles = tmp_path / "bundles"
    for corpus in ("cve", "cwe", "kev"):
        (bundles / "b1" / "corpora" / corpus).mkdir(parents=True)
    tmpdir = tmp_path / "tmp"
    tmpdir.mkdir()
    return {
        "bin": stub_bin,
        "bundles": bundles,
        "tmp": tmpdir,
        "record": tmp_path / "record.json",
    }


def run_entrypoint(
    sandbox: dict[str, Path], args: list[str], **env_overrides: str | None
) -> tuple[subprocess.CompletedProcess[str], int]:
    env = {
        "PATH": f"{sandbox['bin']}:/usr/bin:/bin",
        "BUNDLE_NAME": "b1",
        "BUNDLES_DIR": str(sandbox["bundles"]),
        "FASTRAG_MODEL_DIR": "/models",
        "TMPDIR": str(sandbox["tmp"]),
        "STUB_RECORD": str(sandbox["record"]),
    }
    for key, value in env_overrides.items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    proc = subprocess.Popen(
        ["bash", str(ENTRYPOINT), *args],
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    stdout, stderr = proc.communicate(timeout=30)
    return subprocess.CompletedProcess(
        proc.args, proc.returncode, stdout, stderr
    ), proc.pid


def record(sandbox: dict[str, Path]) -> dict[str, object]:
    return json.loads(sandbox["record"].read_text())


def test_compose_command_runs_under_fastrag_verbatim(sandbox: dict[str, Path]) -> None:
    result, pid = run_entrypoint(sandbox, VAMS_COMMAND)
    assert result.returncode == 0, result.stderr
    got = record(sandbox)
    assert got["name"] == "fastrag"
    assert got["argv"] == VAMS_COMMAND
    # exec, not a child: tini's signals reach fastrag directly.
    assert got["pid"] == pid
    # Inside the container fastrag must listen beyond loopback.
    assert got["fastrag_host"] == "0.0.0.0"


def test_compose_command_keeps_callers_fastrag_host(sandbox: dict[str, Path]) -> None:
    result, _ = run_entrypoint(sandbox, VAMS_COMMAND, FASTRAG_HOST="10.1.2.3")
    assert result.returncode == 0, result.stderr
    assert record(sandbox)["fastrag_host"] == "10.1.2.3"


def test_compose_command_needs_no_bundle_env(sandbox: dict[str, Path]) -> None:
    # The caller's argv names its own bundle (fastrag checks it); the
    # BUNDLE_NAME/BUNDLES_DIR checks belong to the default command only.
    result, _ = run_entrypoint(
        sandbox, ["corpus-info", "--corpus", "/data/c"], BUNDLE_NAME=None
    )
    assert result.returncode == 0, result.stderr
    assert record(sandbox)["argv"] == ["corpus-info", "--corpus", "/data/c"]


def test_executable_on_path_runs_as_given(sandbox: dict[str, Path]) -> None:
    result, pid = run_entrypoint(sandbox, ["other-tool", "--flag", "x y"])
    assert result.returncode == 0, result.stderr
    got = record(sandbox)
    assert got["name"] == "other-tool"
    assert got["argv"] == ["--flag", "x y"]
    assert got["pid"] == pid


def test_explicit_fastrag_is_not_doubled(sandbox: dict[str, Path]) -> None:
    result, _ = run_entrypoint(sandbox, ["fastrag", "serve-http", "--port", "9000"])
    assert result.returncode == 0, result.stderr
    got = record(sandbox)
    assert got["name"] == "fastrag"
    assert got["argv"] == ["serve-http", "--port", "9000"]


def test_leading_flag_goes_to_fastrag(sandbox: dict[str, Path]) -> None:
    result, _ = run_entrypoint(sandbox, ["--version"])
    assert result.returncode == 0, result.stderr
    got = record(sandbox)
    assert got["name"] == "fastrag"
    assert got["argv"] == ["--version"]


def test_no_args_runs_bundle_default(sandbox: dict[str, Path]) -> None:
    result, pid = run_entrypoint(sandbox, [])
    assert result.returncode == 0, result.stderr
    got = record(sandbox)
    argv = got["argv"]
    assert isinstance(argv, list)
    bundle = sandbox["bundles"] / "b1"
    config = Path(argv[argv.index("--config") + 1])
    assert config.parent == sandbox["tmp"]
    assert argv == [
        "serve-http",
        "--corpus",
        f"cve={bundle}/corpora/cve",
        "--corpus",
        f"cwe={bundle}/corpora/cwe",
        "--corpus",
        f"kev={bundle}/corpora/kev",
        "--bundle-path",
        str(bundle),
        "--bundles-dir",
        str(sandbox["bundles"]),
        "--config",
        str(config),
        "--embedder-profile",
        "airgap",
        "--rerank",
        "onnx",
        "--port",
        "8080",
    ]
    assert 'model = "/models/snowflake-arctic-embed-l-Q8_0.GGUF"' in config.read_text()
    assert got["pid"] == pid
    assert got["fastrag_host"] == "0.0.0.0"


def test_no_args_passes_tokens(sandbox: dict[str, Path]) -> None:
    result, _ = run_entrypoint(
        sandbox, [], FASTRAG_TOKEN="read-tok", FASTRAG_ADMIN_TOKEN="admin-tok"
    )
    assert result.returncode == 0, result.stderr
    argv = record(sandbox)["argv"]
    assert argv[-4:] == ["--token", "read-tok", "--admin-token", "admin-tok"]


def test_no_args_without_bundle_name_fails(sandbox: dict[str, Path]) -> None:
    result, _ = run_entrypoint(sandbox, [], BUNDLE_NAME=None)
    assert result.returncode == 1
    assert "BUNDLE_NAME env var required" in result.stderr
    assert not sandbox["record"].exists()
