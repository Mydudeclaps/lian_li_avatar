#!/usr/bin/env python3
"""Add or replace Patch in lian-li-linux's per-user template catalog."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import tempfile
import time
from pathlib import Path


def main() -> None:
    config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    default_generated = (
        config_home / "lianli" / "patch-avatar" / "generated" / "patch-avatar.template.json"
    )
    default_catalog = config_home / "lianli" / "lcd_templates.json"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--template", type=Path, default=default_generated)
    parser.add_argument("--catalog", type=Path, default=default_catalog)
    args = parser.parse_args()

    template = json.loads(args.template.read_text(encoding="utf-8"))
    if not isinstance(template, dict) or not isinstance(template.get("id"), str):
        raise ValueError("rendered template must be an object with a string id")

    if args.catalog.exists():
        catalog = json.loads(args.catalog.read_text(encoding="utf-8"))
        backup = args.catalog.with_name(
            f"{args.catalog.name}.backup-{time.time_ns()}"
        )
        shutil.copy2(args.catalog, backup)
        print(f"Backed up {args.catalog} to {backup}")
    else:
        catalog = {"templates": []}
        args.catalog.parent.mkdir(parents=True, exist_ok=True)

    templates = catalog.get("templates")
    if not isinstance(templates, list):
        raise ValueError("template catalog must contain a templates array")
    catalog["templates"] = [
        existing
        for existing in templates
        if not isinstance(existing, dict) or existing.get("id") != template["id"]
    ]
    catalog["templates"].append(template)

    mode = args.catalog.stat().st_mode & 0o777 if args.catalog.exists() else 0o600
    descriptor, temporary_name = tempfile.mkstemp(
        dir=args.catalog.parent,
        prefix=f".{args.catalog.name}.",
        suffix=".tmp",
        text=True,
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(catalog, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        temporary.chmod(mode)
        temporary.replace(args.catalog)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise
    print(f"Activated template {template['id']} in {args.catalog}")


if __name__ == "__main__":
    main()
