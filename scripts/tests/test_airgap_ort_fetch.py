"""docker/Dockerfile.airgap rust-builder: ONNX Runtime comes from a pinned PyPI wheel.

Microsoft's GitHub release tarballs are gone, so the build takes
libonnxruntime.so.<ver> out of the official onnxruntime wheel. These tests run
the stage's real RUN script under /bin/sh with a stub `curl` that serves a
small wheel, so no image is built and no network is used.
"""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
import zipfile
from pathlib import Path

import pytest

DOCKERFILE = Path(__file__).resolve().parents[2] / "docker" / "Dockerfile.airgap"
VAR = re.compile(r"\$\{(\w+)\}")

STUB_CURL = """#!{python}
import shutil, sys
args = sys.argv[1:]
shutil.copyfile({wheel!r}, args[args.index("-o") + 1])
with open({log!r}, "a") as fh:
    fh.write(next(a for a in args if a.startswith("https://")) + "\\n")
"""


def builder_stage() -> list[str]:
    joined = re.sub(r"\\\n", " ", DOCKERFILE.read_text())
    stage, inside = [], False
    for line in joined.splitlines():
        line = line.strip()
        if line.startswith("FROM "):
            inside = line.endswith(" AS rust-builder")
            continue
        if inside and line and not line.startswith("#"):
            stage.append(line)
    assert stage, "no rust-builder stage in Dockerfile.airgap"
    return stage


def stage_args() -> dict[str, str]:
    return {
        name: default
        for line in builder_stage()
        if line.startswith("ARG ")
        for name, _, default in [line[4:].partition("=")]
    }


def ort_script() -> str:
    runs = [
        line
        for line in builder_stage()
        if "libonnxruntime" in line and line.startswith("RUN")
    ]
    assert len(runs) == 1, runs
    return runs[0][len("RUN ") :]


def run_stage(
    tmp_path: Path, wheel: Path, args: dict[str, str]
) -> subprocess.CompletedProcess:
    bindir = tmp_path / "bin"
    bindir.mkdir()
    log = tmp_path / "curl.log"
    curl = bindir / "curl"
    curl.write_text(
        STUB_CURL.format(python=sys.executable, wheel=str(wheel), log=str(log))
    )
    curl.chmod(0o755)
    out, work = tmp_path / "out", tmp_path / "work"
    work.mkdir()
    script = VAR.sub(lambda m: args.get(m.group(1), m.group(0)), ort_script())
    script = script.replace("/out/", f"{out}/").replace("cd /tmp", f"cd {work}")
    return subprocess.run(
        ["/bin/sh", "-c", script],
        env={"PATH": f"{bindir}:/usr/bin:/bin"},
        capture_output=True,
        text=True,
        check=False,
    )


def make_wheel(path: Path, version: str) -> Path:
    with zipfile.ZipFile(path, "w") as zf:
        zf.writestr(f"onnxruntime/capi/libonnxruntime.so.{version}", b"ELF stub")
        zf.writestr("onnxruntime/__init__.py", b"")
    return path


def test_the_wheel_is_pinned_by_url_and_sha256() -> None:
    args = stage_args()
    assert args["ORT_WHEEL_URL"].startswith("https://files.pythonhosted.org/packages/")
    assert f"onnxruntime-{args['ORT_VERSION']}-" in args["ORT_WHEEL_URL"]
    assert re.fullmatch(r"[0-9a-f]{64}", args["ORT_WHEEL_SHA256"])
    assert "sha256sum -c -" in ort_script()
    assert "onnxruntime/releases/download" not in DOCKERFILE.read_text()


@pytest.mark.skipif(not Path("/usr/bin/unzip").exists(), reason="unzip not installed")
def test_a_matching_wheel_yields_the_library_and_its_links(tmp_path: Path) -> None:
    args = stage_args()
    wheel = make_wheel(tmp_path / "w.whl", args["ORT_VERSION"])
    args["ORT_WHEEL_SHA256"] = hashlib.sha256(wheel.read_bytes()).hexdigest()
    proc = run_stage(tmp_path, wheel, args)
    assert proc.returncode == 0, proc.stderr
    lib = tmp_path / "out/lib" / f"libonnxruntime.so.{args['ORT_VERSION']}"
    assert lib.read_bytes() == b"ELF stub"
    for link in ("libonnxruntime.so.1", "libonnxruntime.so"):
        assert (tmp_path / "out/lib" / link).resolve() == lib.resolve()
    assert (tmp_path / "curl.log").read_text().strip() == args["ORT_WHEEL_URL"]
    assert not (tmp_path / "work/onnxruntime.whl").exists()


@pytest.mark.skipif(not Path("/usr/bin/unzip").exists(), reason="unzip not installed")
def test_a_wheel_with_another_digest_fails_the_build(tmp_path: Path) -> None:
    args = stage_args()
    wheel = make_wheel(tmp_path / "w.whl", args["ORT_VERSION"])
    proc = run_stage(
        tmp_path, wheel, args
    )  # pinned digest is the real wheel's, not this one's
    assert proc.returncode != 0
    assert not (
        tmp_path / "out/lib" / f"libonnxruntime.so.{args['ORT_VERSION']}"
    ).exists()
