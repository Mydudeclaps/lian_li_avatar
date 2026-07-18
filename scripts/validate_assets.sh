#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
animations="$repo_root/assets/mascot/animations"

for command in ffprobe magick; do
  command -v "$command" >/dev/null || {
    echo "missing required command: $command" >&2
    exit 1
  }
done

mapfile -d '' assets < <(find "$animations" -maxdepth 1 -type f -name '*-v3.png' -print0 | sort -z)
if [[ "${#assets[@]}" -ne 27 ]]; then
  echo "expected 27 v3 animation assets; found ${#assets[@]}" >&2
  exit 1
fi

total_frames=0
for asset in "${assets[@]}"; do
  read -r width height frames < <(
    ffprobe -v error -count_frames -select_streams v:0 \
      -show_entries stream=width,height,nb_read_frames \
      -of default=noprint_wrappers=1:nokey=1 "$asset" | paste -sd' ' -
  )
  if [[ "$width" -ne 600 || "$height" -ne 400 || ! "$frames" =~ ^[0-9]+$ || "$frames" -lt 2 ]]; then
    echo "invalid animation geometry/frame count: $asset ($width x $height, $frames frames)" >&2
    exit 1
  fi
  channels="$(magick identify -format '%[channels]' "${asset}[0]")"
  if [[ "$channels" != *a* ]]; then
    echo "animation has no alpha channel: $asset ($channels)" >&2
    exit 1
  fi
  total_frames=$((total_frames + frames))
done

if [[ "$total_frames" -ne 366 ]]; then
  echo "expected 366 animation frames; found $total_frames" >&2
  exit 1
fi
echo "Validated 27 animations, 366 frames, 600x400 RGBA."
