#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/lianli-avatar-tests.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT

for script in "$repo_root"/scripts/*.sh; do
  bash -n "$script"
done

rustc --edition=2021 --test "$repo_root/src/agent_event.rs" -o "$work/agent-event-tests"
"$work/agent-event-tests"
rustc --edition=2021 --test "$repo_root/src/agent_watch.rs" -o "$work/agent-watch-tests"
"$work/agent-watch-tests"

PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$repo_root" \
  python3 -m unittest discover -s "$repo_root/tests" -v

for input in \
  "$repo_root/config/mascot-control.example.json" \
  "$repo_root/config/patch-avatar.template.json.in" \
  "$repo_root/config/hooks/codex-hooks.json.in" \
  "$repo_root/config/hooks/claude-hooks.fragment.json.in"; do
  jq empty "$input"
done

python3 "$repo_root/scripts/render_configs.py" \
  --asset-root "$repo_root/assets" \
  --runtime-dir "$work/runtime" \
  --event-bin "$work/lianli-agent-event" \
  --output-dir "$work/rendered"

for rendered in "$work"/rendered/*.json; do
  jq empty "$rendered"
done

private_scan='/home/'"claps"'|/run/user/'"1000"'|29125d6ad0d28'"200"'|0000:03:'"00.0"'|BEGIN (RSA|OPENSSH|EC) PRIVATE KEY'
if rg -n \
  "$private_scan" \
  "$repo_root" \
  --glob '!.git/**' \
  --glob '!patches/*.patch'; then
  echo "machine-specific or sensitive content found" >&2
  exit 1
fi

echo "Core validation passed."
