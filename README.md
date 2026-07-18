# Patch — Lian Li coding companion

![Patch idle animation](assets/mascot/animations/patch-idle-v3.png)

Patch is a 600×400 animated coding companion for a Lian Li HydroShift II
Curved LCD. He notices Codex and Claude Code sessions, reacts to their actual
activity, watches CPU/GPU/coolant temperatures, performs little ambient
animations, and roams across the full physical screen in both axes.

This repository is the portable, public-safe source release of the project
running on the original machine.

## What is included

- 27 final transparent APNGs: 366 frames of idle, work, wait, enter/exit,
  tool/read, success/error, thermal, and left/right walking states.
- Corrected six-frame walking art, v3 pose sheets, and the deterministic
  ImageMagick/FFmpeg animation builder.
- Standalone Rust event writer and watcher with 32 inline tests.
- Codex and Claude Code hook templates that emit only allowlisted lifecycle
  events and discard hook input.
- Full 3×3 movement, horizontal/vertical/full roaming patterns, three speeds,
  manual positioning, previews, and reaction toggles.
- A focused Grand Line-style touch control page and safe loopback Python
  bridge, with no third-party dependencies.
- The minimal patch needed to add selector variants, transparent crossfades,
  and two-axis movement to `sgtaziz/lian-li-linux`.

The local anime background is intentionally not included because it did not
have clear redistribution provenance. The supplied LCD template uses a black
background and the original telemetry panels; add a background you have the
right to use if desired.

## How it works

Codex/Claude hook events, Linux process/cgroup activity, temperatures, and
touchscreen commands feed one watcher. The watcher chooses a state by priority
and writes tiny files under `$XDG_RUNTIME_DIR`. The patched renderer polls
those files, crossfades the matching APNG, and smoothly interpolates the
mascot's logical screen position.

The hooks never capture prompts, code, tool arguments, or model output.
[Architecture](docs/ARCHITECTURE.md) and [security notes](docs/SECURITY.md)
describe the boundaries in detail.

## Requirements

- Linux with `/proc`, cgroup v2, and a systemd user session
- `rustc`, Python 3, and `jq`
- `ffmpeg` and ImageMagick 7 (`magick`) to validate or rebuild animations
- a compatible checkout of
  [`sgtaziz/lian-li-linux`](https://github.com/sgtaziz/lian-li-linux)
- a 2288×1048 logical HydroShift II Curved template layout

The watcher defaults to `k10temp/Tctl`, `amdgpu/edge`, and automatic discovery
of `$XDG_RUNTIME_DIR/lianli-coolant-*`. Override sensor discovery with:

```text
LIANLI_CPU_HWMON_ROOT
LIANLI_CPU_HWMON_NAME
LIANLI_CPU_TEMP_LABEL
LIANLI_GPU_HWMON_ROOT
LIANLI_GPU_HWMON_NAME
LIANLI_GPU_TEMP_LABEL
LIANLI_COOLANT_FILE
```

`LIANLI_COOLANT_FILE` is a basename inside the trusted runtime directory.

## Install

First apply and build the renderer patch. It targets upstream commit
`d262007c9bfbe87ae7c9d390d68ec74e5deb4d0a`:

```bash
git clone https://github.com/sgtaziz/lian-li-linux
cd lian-li-linux
git checkout d262007c9bfbe87ae7c9d390d68ec74e5deb4d0a
git apply --check /path/to/lian_li_avatar/patches/lian-li-linux-avatar.patch
git apply /path/to/lian_li_avatar/patches/lian-li-linux-avatar.patch
cargo test -p lianli-shared -p lianli-media
```

Build/install that daemon using its upstream instructions. Then, from this
repository:

```bash
./scripts/install.sh --activate-template
```

The installer:

1. copies Patch's runtime and authoring assets under
   `~/.config/lianli/templates/patch-avatar`;
2. compiles/tests and installs the event writer and watcher;
3. installs/enables `lianli-agent-watch.service`;
4. renders real home/runtime paths into private template and hook examples;
5. optionally backs up and activates `patch-avatar` in `lcd_templates.json`.

Merge the generated hook files from
`~/.config/lianli/patch-avatar/generated/` deliberately:

- `codex-hooks.json` is a Codex hook object;
- `claude-hooks.fragment.json` contains only the `.hooks` fragment and must be
  merged while preserving every other Claude setting.

Review the Codex hooks with `/hooks`, reopen existing agent sessions, then
restart the Lian Li daemon once so it preloads the template and animation
frames. State and position changes do not require daemon restarts.

## Controls

The development CLI can inspect, preview, or force behavior:

```bash
lianli-mascot status
lianli-mascot auto
lianli-mascot demo
lianli-mascot codex
lianli-mascot event-error
lianli-mascot thermal-hot
lianli-mascot move upper-left 2400
lianli-mascot move right 1800
lianli-mascot move home 1200
```

Forcing a state or destination with this CLI stops automatic detection;
`lianli-mascot auto` returns control to the watcher.

Run the focused touch control page with:

```bash
python3 -m grand_line.server
```

Then open `http://127.0.0.1:7071`. The server is deliberately loopback-only.
Its Python bridge can also be embedded into an existing local dashboard; the
API contract is covered by the tests under `tests/`.

## Animation development

Rebuild every v3 animation from the checked-in poses:

```bash
./scripts/build_animations.sh
./scripts/validate_assets.sh
```

The builder checks frame counts, 600×400 geometry, alpha presence, and opaque
black poster frames—the original cause of the jump flash. Image-generation
prompts and working-sheet provenance are preserved in
[ASSET_PROVENANCE.md](docs/ASSET_PROVENANCE.md).

## Validation

```bash
make test
make test-assets
```

`make test` runs 32 Rust tests, the Python control/HTTP tests, JSON rendering,
shell syntax checks, and a scan for machine-specific paths and identifiers.

## Attribution

The renderer patch modifies MIT-licensed upstream code; its upstream license
is included beside the patch. No blanket license has been assigned to the
original Patch code or artwork in this repository.

Patch is a fan-made original mascot. This project is not affiliated with or
endorsed by Lian Li, OpenAI, Anthropic, or any manga/anime publisher.
