#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="$repo_root/src/agent_watch.rs"
destination="${HOME}/.local/bin/lianli-agent-watch"
destination_dir="$(dirname -- "$destination")"
runtime_dir="${XDG_RUNTIME_DIR:-/tmp}"

install -d -m 755 -- "$destination_dir"
temporary="$(mktemp --tmpdir="$destination_dir" .lianli-agent-watch.XXXXXX)"
test_binary="$(mktemp --tmpdir="$runtime_dir" lianli-agent-watch-tests.XXXXXX)"

cleanup() {
  rm -f -- "$temporary" "$test_binary"
}
trap cleanup EXIT

rustc --edition=2021 --test "$source_file" -o "$test_binary"
"$test_binary"

rustc --edition=2021 -C opt-level=2 -C strip=symbols "$source_file" -o "$temporary"
chmod 755 "$temporary"
mv -f -- "$temporary" "$destination"

echo "Built $destination"
if systemctl --user --quiet is-active lianli-agent-watch.service; then
  systemctl --user restart lianli-agent-watch.service
  echo "Restarted lianli-agent-watch.service"
fi
