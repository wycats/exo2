#!/usr/bin/env bash
set -euo pipefail

repo="${EXO_ARTIFACT_REPO:-wycats/exo2}"
branch="${EXO_ARTIFACT_BRANCH:-main}"
workflow="${EXO_ARTIFACT_WORKFLOW:-exo-binaries.yml}"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
install_dir="${EXO_INSTALL_DIR:-$cargo_home/bin}"
run_id="${EXO_ARTIFACT_RUN_ID:-}"

if ! command -v gh >/dev/null 2>&1; then
  echo "error: GitHub CLI 'gh' is required to download Exo binary artifacts" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required to select and verify Exo binary artifacts" >&2
  exit 1
fi

if [[ -z "$run_id" ]]; then
  run_id="$(
    gh run list \
      --repo "$repo" \
      --workflow "$workflow" \
      --branch "$branch" \
      --event push \
      --status success \
      --limit 1 \
      --json databaseId \
      --jq '.[0].databaseId'
  )"
fi

if [[ -z "$run_id" || "$run_id" == "null" ]]; then
  echo "error: no successful '$workflow' push run found for $repo on branch $branch" >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

echo "Downloading Exo binary artifacts from $repo run $run_id..."
gh run download "$run_id" --repo "$repo" --dir "$tmpdir"

python3 - "$tmpdir" "$install_dir" <<'PY'
import hashlib
import json
import os
import platform
import shutil
import stat
import sys
import tempfile
from pathlib import Path

download_dir = Path(sys.argv[1])
install_dir = Path(sys.argv[2]).expanduser()


def normalized_platform() -> str:
    system = platform.system().lower()
    if system == "darwin":
        return "macos"
    if system == "linux":
        return "linux"
    if system == "windows":
        return "windows"
    return system


def normalized_arch() -> str:
    machine = platform.machine().lower()
    if machine in {"amd64", "x86_64"}:
        return "x86_64"
    if machine in {"arm64", "aarch64"}:
        return "aarch64"
    return machine


def verify_binary(path: Path, expected_sha256: str) -> None:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != expected_sha256:
        raise SystemExit(
            f"error: checksum mismatch for {path.name}: expected {expected_sha256}, got {digest}"
        )


def install_binary(source: Path, target: Path) -> None:
    temp_path = None
    fd, temp_name = tempfile.mkstemp(
        prefix=f".{target.name}.",
        suffix=".tmp",
        dir=str(target.parent),
    )
    os.close(fd)
    temp_path = Path(temp_name)

    try:
        shutil.copy2(source, temp_path)
        temp_path.chmod(
            temp_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH
        )
        os.replace(temp_path, target)
    finally:
        if temp_path is not None and temp_path.exists():
            temp_path.unlink()


target_platform = normalized_platform()
target_arch = normalized_arch()
manifests = []

for manifest_path in download_dir.glob("*/manifest.json"):
    manifest = json.loads(manifest_path.read_text())
    manifests.append((manifest_path, manifest))

for manifest_path, manifest in manifests:
    if manifest.get("platform") != target_platform or manifest.get("arch") != target_arch:
        continue

    artifact_dir = manifest_path.parent
    binaries = {entry["name"]: entry for entry in manifest.get("binaries", [])}
    exo_name = next((name for name in binaries if name in {"exo", "exo.exe"}), None)
    exo_mcp_name = next((name for name in binaries if name in {"exo-mcp", "exo-mcp.exe"}), None)

    if exo_name is None or exo_mcp_name is None:
        raise SystemExit(f"error: artifact {artifact_dir} does not contain both exo and exo-mcp")

    install_entries = [
        (exo_name, binaries[exo_name]),
        (exo_mcp_name, binaries[exo_mcp_name]),
    ]

    for name, entry in install_entries:
        source = artifact_dir / name
        verify_binary(source, entry["sha256"])

    install_dir.mkdir(parents=True, exist_ok=True)

    for name, _entry in install_entries:
        source = artifact_dir / name
        target = install_dir / name
        install_binary(source, target)
        print(f"installed {target}")

    print(
        "installed Exo binaries from "
        f"{manifest.get('git_sha')} ({manifest.get('platform')}/{manifest.get('arch')})"
    )
    raise SystemExit(0)

available = [
    f"{manifest.get('platform')}/{manifest.get('arch')} from {path.parent.name}"
    for path, manifest in manifests
]
available_text = "\n  - ".join(available) if available else "none"
raise SystemExit(
    "error: no Exo binary artifact matched this host "
    f"({target_platform}/{target_arch}). Available artifacts:\n  - {available_text}"
)
PY
