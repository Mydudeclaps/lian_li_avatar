#!/usr/bin/env python3
"""Render machine-specific paths into Patch's JSON configuration templates."""

from __future__ import annotations

import argparse
import json
import os
import re
import tempfile
from pathlib import Path


PLACEHOLDER = re.compile(r"@[A-Z_]+@")


def atomic_write(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f".{path.name}.",
        suffix=".tmp",
        text=True,
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.chmod(0o600)
        temporary.replace(path)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise


def render(source: Path, destination: Path, replacements: dict[str, str]) -> None:
    value = source.read_text(encoding="utf-8")
    for placeholder, replacement in replacements.items():
        value = value.replace(placeholder, replacement)
    unresolved = sorted(set(PLACEHOLDER.findall(value)))
    if unresolved:
        raise ValueError(f"{source}: unresolved placeholders: {', '.join(unresolved)}")
    json.loads(value)
    atomic_write(destination, value)


def absolute_path(value: str, label: str) -> str:
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise ValueError(f"{label} must be an absolute path")
    return str(path)


def main() -> None:
    repository = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset-root", required=True)
    parser.add_argument("--runtime-dir", required=True)
    parser.add_argument("--event-bin", required=True)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()

    output = Path(absolute_path(args.output_dir, "output directory"))
    replacements = {
        "@ASSET_ROOT@": absolute_path(args.asset_root, "asset root"),
        "@RUNTIME_DIR@": absolute_path(args.runtime_dir, "runtime directory"),
        "@EVENT_BIN@": absolute_path(args.event_bin, "event binary"),
    }
    jobs = (
        (
            repository / "config" / "patch-avatar.template.json.in",
            output / "patch-avatar.template.json",
        ),
        (
            repository / "config" / "hooks" / "codex-hooks.json.in",
            output / "codex-hooks.json",
        ),
        (
            repository / "config" / "hooks" / "claude-hooks.fragment.json.in",
            output / "claude-hooks.fragment.json",
        ),
    )
    for source, destination in jobs:
        render(source, destination, replacements)
        print(f"Rendered {destination}")


if __name__ == "__main__":
    main()
