#!/usr/bin/env bash
set -euo pipefail
umask 077

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
config_root="$config_home/lianli"
asset_root="${LIANLI_AVATAR_ASSET_ROOT:-$config_root/templates/patch-avatar}"
generated_root="$config_root/patch-avatar/generated"
runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
bin_root="$HOME/.local/bin"
activate_template=false

if [[ "${1:-}" == "--activate-template" ]]; then
  activate_template=true
elif [[ $# -ne 0 ]]; then
  echo "usage: $0 [--activate-template]" >&2
  exit 2
fi

for command in rustc python3 install systemctl; do
  command -v "$command" >/dev/null || {
    echo "missing required command: $command" >&2
    exit 1
  }
done
[[ -d "$runtime_dir" ]] || {
  echo "trusted user runtime directory is missing: $runtime_dir" >&2
  exit 1
}

install -d -m 755 -- \
  "$asset_root/mascot" \
  "$asset_root/display" \
  "$bin_root" \
  "$config_home/systemd/user"
cp -a -- "$repo_root/assets/mascot/." "$asset_root/mascot/"
# Locally branded animations (official agent icons composited by
# build_animations.sh; never committed) take precedence when present.
rm -rf -- "$asset_root/mascot/animations-branded"
branded_animations="$repo_root/assets/mascot/animations-branded"
if [[ -d "$branded_animations" ]]; then
  echo "Installing locally branded animations from $branded_animations"
  cp -a -- "$branded_animations/." "$asset_root/mascot/animations/"
fi
install -m 644 -- "$repo_root/assets/display/panels.png" "$asset_root/display/panels.png"
install -m 755 -- "$repo_root/scripts/lianli-mascot" "$bin_root/lianli-mascot"
install -m 644 -- \
  "$repo_root/systemd/lianli-agent-watch.service" \
  "$config_home/systemd/user/lianli-agent-watch.service"

if [[ ! -e "$config_root/mascot-control.json" ]]; then
  install -d -m 700 -- "$config_root"
  install -m 600 -- \
    "$repo_root/config/mascot-control.example.json" \
    "$config_root/mascot-control.json"
fi

"$repo_root/scripts/build_agent_event.sh"
"$repo_root/scripts/build_agent_watch.sh"

python3 "$repo_root/scripts/render_configs.py" \
  --asset-root "$asset_root" \
  --runtime-dir "$runtime_dir" \
  --event-bin "$bin_root/lianli-agent-event" \
  --output-dir "$generated_root"

systemctl --user daemon-reload
systemctl --user enable --now lianli-agent-watch.service

if "$activate_template"; then
  python3 "$repo_root/scripts/activate_template.py" \
    --template "$generated_root/patch-avatar.template.json"
fi

echo
echo "Patch core is installed."
echo "Rendered template and hook examples: $generated_root"
if ! "$activate_template"; then
  echo "Activate the LCD template with:"
  echo "  python3 $repo_root/scripts/activate_template.py"
fi
echo "Merge the generated hook JSON deliberately; do not overwrite unrelated settings."
echo "The renderer patch must be installed before selecting the Patch LCD template."
