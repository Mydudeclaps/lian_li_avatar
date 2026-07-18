#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
root="$repo_root/assets/mascot"
legacy="$root/poses"
poses="$root/poses-v3"
animations="$root/animations"
work="$(mktemp -d "${TMPDIR:-/tmp}/patch-animations-v3.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT

width=600
height=400
delay_cs=10

mkdir -p "$animations"

frame_path() {
  printf '%s/%03d.png' "$1" "$2"
}

render_asset() {
  local input="$1"
  local target_height="$2"
  local center_x="$3"
  local bottom_padding="$4"
  local flop="$5"
  local output="$6"
  shift 6

  local geometry
  printf -v geometry '%+d%+d' "$center_x" "$bottom_padding"

  local -a transform=()
  if [[ "$flop" == "true" ]]; then
    transform+=(-flop)
  fi

  # Every frame is explicitly RGBA. This prevents a transparent poster frame
  # from being decoded later as opaque black.
  magick \
    -size "${width}x${height}" canvas:none \
    \( "$input" -trim +repage "${transform[@]}" -resize "x${target_height}" \) \
    -gravity south -geometry "$geometry" -compose over -composite \
    "$@" \
    -depth 8 -define png:color-type=6 "PNG32:$output"
}

render_legacy() {
  local name="$1"
  shift
  render_asset "$legacy/$name.png" "$@"
}

render_pose() {
  local name="$1"
  shift
  render_asset "$poses/$name-v3.png" "$@"
}

render_walk() {
  local index="$1"
  local center_x="$2"
  local bottom_padding="$3"
  local direction="$4"
  local output="$5"
  local flop=false
  if [[ "$direction" == "left" ]]; then
    flop=true
  fi
  render_asset "$legacy/walk-v2-0${index}.png" 270 "$center_x" "$bottom_padding" "$flop" "$output"
}

encode_apng() {
  local input_dir="$1"
  local output="$2"
  magick -delay "$delay_cs" -loop 0 "$input_dir"/*.png "APNG:$output"
}

validate_asset() {
  local asset="$1"
  local expected_frames="$2"
  local decoded="$work/validate-$(basename "$asset" .png)"
  mkdir -p "$decoded"

  ffmpeg -hide_banner -loglevel error -y -i "$asset" -vsync 0 "$decoded/%03d.png"
  local actual_frames
  actual_frames="$(find "$decoded" -maxdepth 1 -name '*.png' -printf '.' | wc -c)"
  if [[ "$actual_frames" -ne "$expected_frames" ]]; then
    echo "validation failed: $asset has $actual_frames frames; expected $expected_frames" >&2
    return 1
  fi

  for frame in "$decoded"/*.png; do
    read -r frame_width frame_height < <(magick identify -format '%w %h\n' "$frame")
    if [[ "$frame_width" -ne "$width" || "$frame_height" -ne "$height" ]]; then
      echo "validation failed: $frame is ${frame_width}x${frame_height}" >&2
      return 1
    fi
    local alpha_max
    alpha_max="$(magick "$frame" -alpha extract -format '%[fx:maxima]' info:)"
    if [[ "$alpha_max" == "0" ]]; then
      echo "validation failed: $frame is fully transparent" >&2
      return 1
    fi
  done

  local first="$decoded/001.png"
  local alpha_mean rgb_max
  alpha_mean="$(magick "$first" -alpha extract -format '%[fx:mean]' info:)"
  rgb_max="$(magick "$first" -alpha off -format '%[fx:maxima]' info:)"
  if [[ "$alpha_mean" == "1" && "$rgb_max" == "0" ]]; then
    echo "validation failed: $asset begins with opaque black" >&2
    return 1
  fi
}

make_idle() {
  local dir="$work/idle"
  mkdir -p "$dir"
  local i x bob
  for ((i=0; i<12; i++)); do
    x=$((i == 5 || i == 6 ? 2 : 0))
    bob=$((i >= 3 && i <= 7 ? 10 : 8))
    render_legacy idle 365 "$x" "$bob" false "$(frame_path "$dir" "$i")"
  done
  encode_apng "$dir" "$animations/patch-idle-v3.png"
  validate_asset "$animations/patch-idle-v3.png" 12
}

make_enter() {
  local dir="$work/enter"
  mkdir -p "$dir"
  local i pose x bob
  for ((i=0; i<24; i++)); do
    if ((i < 18)); then
      pose=$((i % 6 + 1))
      # Begin with the hat just inside the widget. A fully transparent first
      # APNG frame is technically valid but has triggered opaque poster-frame
      # bugs in several decoder paths.
      x=$((-275 + i * 16))
      bob=$((i % 6 == 1 || i % 6 == 4 ? 8 : 12))
      render_walk "$pose" "$x" "$bob" right "$(frame_path "$dir" "$i")"
    else
      bob=$((i == 19 || i == 20 ? 5 : 8))
      render_legacy idle 365 0 "$bob" false "$(frame_path "$dir" "$i")"
    fi
  done
  encode_apng "$dir" "$animations/patch-enter-v3.png"
  validate_asset "$animations/patch-enter-v3.png" 24
}

make_exit() {
  local dir="$work/exit"
  mkdir -p "$dir"
  local i pose x bob
  for ((i=0; i<30; i++)); do
    if ((i < 6)); then
      bob=$((i == 2 || i == 3 ? 8 : 10))
      render_pose wave 350 0 "$bob" false "$(frame_path "$dir" "$i")"
    elif ((i < 12)); then
      bob=$((i == 8 || i == 9 ? 7 : 10))
      render_pose hat-tip 350 0 "$bob" false "$(frame_path "$dir" "$i")"
    else
      pose=$(((i - 12) % 6 + 1))
      # Finish with one boot/coat edge still visible; the selector crossfade
      # completes the departure without an empty APNG poster frame.
      x=$(((i - 12) * 21))
      bob=$((pose == 2 || pose == 5 ? 8 : 12))
      render_walk "$pose" "$x" "$bob" right "$(frame_path "$dir" "$i")"
    fi
  done
  encode_apng "$dir" "$animations/patch-exit-v3.png"
  validate_asset "$animations/patch-exit-v3.png" 30
}

make_legacy_loop() {
  local output_name="$1"
  local pose="$2"
  local target_height="$3"
  local accent="$4"
  local frames="${5:-14}"
  local dir="$work/$output_name"
  mkdir -p "$dir"
  local i x bob
  for ((i=0; i<frames; i++)); do
    x=0
    bob=$((8 + (i % 7 == 2 || i % 7 == 3 ? 3 : 0)))
    if ((i == 5 || i == 11)); then
      x=3
    fi
    case "$accent" in
      cyan)
        render_legacy "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#63e6ff' -draw "rectangle 92,74 99,84 rectangle 493,111 501,122"
        ;;
      amber)
        render_legacy "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#ffbf5b' -draw "circle 95,109 99,109 circle 500,78 504,78"
        ;;
      dual)
        render_legacy "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#63e6ff' -draw "rectangle 84,92 91,103" \
          -fill '#ffbf5b' -draw "circle 507,104 512,104"
        ;;
      *)
        render_legacy "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")"
        ;;
    esac
  done
  encode_apng "$dir" "$animations/patch-$output_name-v3.png"
  validate_asset "$animations/patch-$output_name-v3.png" "$frames"
}

make_pose_loop() {
  local output_name="$1"
  local pose="$2"
  local target_height="$3"
  local accent="$4"
  local frames="${5:-12}"
  local dir="$work/$output_name"
  mkdir -p "$dir"
  local i x bob
  for ((i=0; i<frames; i++)); do
    x=0
    bob=$((8 + (i % 6 == 2 || i % 6 == 3 ? 3 : 0)))
    case "$accent" in
      cyan)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#63e6ff' -draw "rectangle 80,68 88,80 rectangle 503,125 511,137"
        ;;
      amber)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#ffbf5b' -draw "circle 91,99 96,99 circle 505,72 510,72"
        ;;
      yellow)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#ffd84d' -draw "rectangle 74,67 83,82 rectangle 511,93 520,108"
        ;;
      red)
        x=$((i % 4 == 1 ? -3 : i % 4 == 3 ? 3 : 0))
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#ff5d73' -draw "rectangle 74,96 83,110 rectangle 511,67 520,81"
        ;;
      gold)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#ffd166' -draw "rectangle 76,65 86,80 rectangle 506,105 516,120"
        ;;
      blue)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")" \
          -fill '#75d8ff' -draw "circle 87,85 93,85 circle 514,111 520,111"
        ;;
      *)
        render_pose "$pose" "$target_height" "$x" "$bob" false "$(frame_path "$dir" "$i")"
        ;;
    esac
  done
  encode_apng "$dir" "$animations/patch-$output_name-v3.png"
  validate_asset "$animations/patch-$output_name-v3.png" "$frames"
}

make_blink() {
  local dir="$work/idle-blink"
  mkdir -p "$dir"
  local i
  for ((i=0; i<8; i++)); do
    if ((i == 3 || i == 4)); then
      render_pose blink 365 0 8 false "$(frame_path "$dir" "$i")"
    else
      render_legacy idle 365 0 8 false "$(frame_path "$dir" "$i")"
    fi
  done
  encode_apng "$dir" "$animations/patch-idle-blink-v3.png"
  validate_asset "$animations/patch-idle-blink-v3.png" 8
}

make_stroll() {
  local direction="$1"
  local dir="$work/stroll-$direction"
  mkdir -p "$dir"
  local i pose bob
  for ((i=0; i<12; i++)); do
    pose=$((i % 6 + 1))
    bob=$((pose == 2 || pose == 5 ? 8 : 12))
    render_walk "$pose" 0 "$bob" "$direction" "$(frame_path "$dir" "$i")"
  done
  encode_apng "$dir" "$animations/patch-stroll-$direction-v3.png"
  validate_asset "$animations/patch-stroll-$direction-v3.png" 12
}

make_done() {
  local dir="$work/event-done"
  mkdir -p "$dir"
  local i
  for ((i=0; i<12; i++)); do
    if ((i < 6)); then
      render_pose wave 350 0 10 false "$(frame_path "$dir" "$i")"
    else
      render_pose hat-tip 350 0 10 false "$(frame_path "$dir" "$i")"
    fi
  done
  encode_apng "$dir" "$animations/patch-event-done-v3.png"
  validate_asset "$animations/patch-event-done-v3.png" 12
}

make_critical() {
  local dir="$work/thermal-critical"
  mkdir -p "$dir"
  local i x
  for ((i=0; i<12; i++)); do
    x=$((i % 4 == 1 ? -4 : i % 4 == 3 ? 4 : 0))
    render_pose thermal-hot 350 "$x" 9 false "$(frame_path "$dir" "$i")" \
      -fill '#ff5364' -draw "rectangle 62,58 73,76 rectangle 520,80 531,98"
  done
  encode_apng "$dir" "$animations/patch-thermal-critical-v3.png"
  validate_asset "$animations/patch-thermal-critical-v3.png" 12
}

make_idle
make_enter
make_exit

make_legacy_loop codex-active typing 355 cyan 12
make_legacy_loop claude-active reading 355 amber 14
make_legacy_loop both-active dual 355 dual 12
make_legacy_loop codex-wait thinking 355 cyan 16
make_legacy_loop claude-wait reading 350 amber 16
make_legacy_loop both-wait dual 350 dual 16

make_blink
make_pose_loop idle-hat-adjust hat-adjust 355 none 10
make_pose_loop idle-stretch stretch 355 none 12
make_pose_loop idle-mug mug 335 amber 14
make_pose_loop idle-gauge gauge 345 yellow 12
make_pose_loop idle-breeze breeze 345 blue 12
make_stroll left
make_stroll right

make_pose_loop event-tool tool 345 cyan 12
make_pose_loop event-reading reading 350 amber 12
make_pose_loop event-attention attention 350 yellow 10
make_pose_loop event-error error 350 red 12
make_pose_loop event-success success 350 gold 16
make_done

make_pose_loop thermal-warm gauge 345 yellow 12
make_pose_loop thermal-hot thermal-hot 350 blue 12
make_critical
make_pose_loop thermal-relief thermal-relief 350 blue 12

echo "Built and validated Patch v3 assets:"
identify "$animations"/patch-*-v3.png
