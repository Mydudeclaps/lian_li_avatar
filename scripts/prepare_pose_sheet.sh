#!/usr/bin/env bash
# Split a flat chroma-green (#00FF00) pose sheet into transparent per-pose
# PNGs ready for build_animations.sh.
#
# Usage:
#   prepare_pose_sheet.sh <sheet.png> <COLSxROWS> <output-dir> <prefix> [fuzz%]
#
# Example:
#   prepare_pose_sheet.sh art_sprint/navigator-core-sheet-v1.png 4x2 \
#       assets/mascot/poses-navigator navigator-core 10
#
# The fuzz percentage controls how far from pure chroma green a pixel may be
# and still become transparent; raise it slightly if generated sheets carry
# compression noise, lower it if a character's own greens start keying out.
set -euo pipefail

if [[ $# -lt 4 || $# -gt 5 ]]; then
  echo "usage: $0 <sheet.png> <COLSxROWS> <output-dir> <prefix> [fuzz%]" >&2
  exit 2
fi

sheet="$1"
grid="$2"
output_dir="$3"
prefix="$4"
fuzz="${5:-10}"

cols="${grid%x*}"
rows="${grid#*x}"
if ! [[ "$cols" =~ ^[1-9][0-9]?$ && "$rows" =~ ^[1-9][0-9]?$ ]]; then
  echo "grid must look like 4x2, got '$grid'" >&2
  exit 2
fi
if ! [[ "$fuzz" =~ ^[0-9]{1,2}$ ]]; then
  echo "fuzz must be an integer percentage (0-99), got '$fuzz'" >&2
  exit 2
fi
if [[ ! -f "$sheet" ]]; then
  echo "sheet not found: $sheet" >&2
  exit 2
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/prepare-pose-sheet.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
mkdir -p "$output_dir"

magick "$sheet" -crop "${cols}x${rows}@" +repage +adjoin "$work/cell_%02d.png"

expected=$((cols * rows))
produced=0
index=0
for cell in "$work"/cell_*.png; do
  index=$((index + 1))
  output="$output_dir/$(printf '%s-%02d.png' "$prefix" "$index")"
  # Key out chroma green, then despill: pixels that stay strongly
  # green-dominant (chroma contamination on edges and shadow remnants) have
  # their green clamped to max(red, blue). The 1.4 dominance threshold
  # leaves legitimate colors such as Patch's teal scarf untouched.
  # Every pose is explicitly RGBA (PNG32) so a later decode can never treat
  # transparency as opaque black — the original cause of the jump flash.
  magick "$cell" -alpha on -fuzz "${fuzz}%" -transparent '#00FF00' \
    -channel G -fx '(g>1.4*r && g>1.4*b) ? max(r,b) : g' +channel \
    -depth 8 -define png:color-type=6 "PNG32:$output"

  opaque="$(magick "$output" -format '%[opaque]' info:)"
  if [[ "$opaque" != "False" ]]; then
    echo "error: $output has no transparency; chroma key failed" >&2
    exit 1
  fi
  produced=$((produced + 1))
done

if [[ "$produced" -ne "$expected" ]]; then
  echo "error: expected $expected cells, produced $produced" >&2
  exit 1
fi

echo "Prepared $produced poses under $output_dir with prefix '$prefix'."
