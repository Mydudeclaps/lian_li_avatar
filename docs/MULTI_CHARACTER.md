# Multi-character design (v2 protocol)

Status: migration steps 1–2 are implemented. The watcher drives a roster of
per-character engines, writes character-suffixed state/position/control-status
files alongside the un-suffixed v1 files for `patch`, accepts v2 five-field
commands (`v1 <ms> <character|-> <command> <argument>`), and routes events by
actor mask. The control bridge takes an optional allowlisted `character`
field and reports per-character status in its snapshot. The `navigator`
roster entry (Codex-routed, thermal reactions off, home lower-left) is
enabled by listing her in the `characters` array of mascot-control.json, and
her v1 animation set builds from `assets/mascot/poses-navigator/`; see
`config/navigator-widget.example.json.in` for her template widget.

The separation policy ships as distinct home anchors plus the roam
reservation: a character never starts a roam toward a waypoint within
500 px of another character's resting point or walk destination — it
re-rolls once and otherwise gives the ambient slot back; mid-flight
crossings stay allowed. The offset waypoint grids from the original
draft were dropped deliberately: the shared 3×3 grid already spans the
full envelope that keeps a 600×400 sprite on screen, so any fixed shift
either clips sprites at the edge columns or collapses back onto the
shared points, and the reservation covers the conflicts the offsets were
meant to prevent.

Per-character settings ship as `character_settings` sections in
mascot-control.json (API key `characterSettings`): a crewmate section holds
a subset of the global setting keys and overrides only those; absent keys
inherit live. The default character has no section — the top-level settings
are his.

Not yet implemented from this design: dropping the v1 compatibility output
behind a config flag.

## Goal

Two or three independent characters roaming the same 2288×1048 logical
canvas, each with its own animations, personality, and reactions, without
renderer patch changes and within a measured budget of roughly 1–3 % of one
CPU core and 30–60 MB RSS per additional character.

Non-goals: characters on separate devices, characters that overlap or
physically interact with pixel precision, and any change to the v1 record
formats themselves.

## Why the renderer already supports this

Every selector, crossfade, decoded-frame cache, and position interpolation in
the renderer patch is per-widget state. A second character is simply a second
`video` widget whose `selector.state_file` and `selector.position_file` point
at different runtime files. The only renderer-side improvement worth
considering later is a process-wide clip cache so characters sharing art do
not duplicate encoded frame stores; it is an optimization, not a requirement.

## Character roster

Proposed initial cast (art pending):

| id          | reacts to                    | home       | roam default |
|-------------|------------------------------|------------|--------------|
| `patch`     | Claude Code + fallback: both | lower mid  | full         |
| `navigator` | Codex                        | lower left | horizontal   |
| `doctor`    | thermals + coolant           | lower right| vertical     |

Routing is configuration, not code: each character declares an actor mask
(`codex`, `claude`, `both`, `none`) and reaction toggles. With `navigator`
absent, `patch` keeps today's behavior of reacting to both agents, so a
one-character install remains exactly the current system.

## v2 runtime contract

File names become character-addressed; record formats stay v1:

```text
lianli-agent-state-<id>
lianli-agent-position-<id>
lianli-agent-control-status-<id>
lianli-agent-command.<created-ms>.<pid>      (queued; gains a character field)
lianli-agent-event-<actor>.<created-ms>.<pid> (unchanged; actor-addressed)
```

- Character ids are lowercase `[a-z][a-z0-9-]{0,15}`, allowlisted from the
  roster configuration.
- Events stay actor-addressed. The watcher routes each consumed event to
  every character whose mask includes that actor. A third character reacting
  to no agent (e.g. `doctor`) simply has mask `none`.
- The command record grows one field:
  `v1 <created-ms> <character|-> <command> <argument>`, where `-` targets the
  default character. The parser continues to reject unknown fields, so v1
  four-field records remain valid during migration and are treated as
  addressed to `patch`.
- The watcher keeps writing the un-suffixed v1 files for `patch` until the
  template is migrated, then drops them behind a config flag.

## Watcher architecture

`StateEngine` already contains all per-character state (animation, events,
thermal reactions, ambient, manual control, RNG, sequence numbers). The
change is a roster:

```text
struct Character {
    id: String,
    actor_mask: u8,
    thermal_reactions: bool,
    engine: StateEngine,
    paths: CharacterPaths,   // state/position/control-status
    home: Waypoint,
    waypoints: &'static [Waypoint],  // offset grid, see below
}
```

Signal collection (process scan, cgroups, temperatures) stays global and runs
once; `EngineInput` is assembled per character by masking `codex_active` /
`claude_active` / events / thermal samples through the character's routing.
Each engine gets a distinct RNG seed (hash of the shared seed and the id) so
ambient timing never synchronizes. Cost: the engine step is microseconds;
three engines are free next to the 1-second `/proc` scan.

## Separation policy (keeping 600×400 sprites apart)

Simple and deterministic, in priority order:

1. Distinct home anchors (table above) — idle characters never stack.
2. Offset waypoint grids: each character's 3×3 grid is shifted by a
   per-character offset (±220 px horizontally) so shared route names resolve
   to disjoint points.
3. Roam reservation: before starting a roam, a character checks the other
   characters' current targets; if the chosen waypoint's center is within
   500 px of another character's target or resting point, it re-rolls once and
   otherwise skips this ambient slot. No mid-flight avoidance — walks are
   short and crossing paths is charming rather than wrong.

All of this lives in the watcher and is unit-testable with the existing
engine test harness.

## Control bridge and touch page

- `PatchControl` gains a `character` parameter (allowlisted against the
  roster) on `send_command`; settings become per-character sections with the
  current global keys as the `patch` defaults.
- `snapshot()` returns `{"characters": {<id>: {mode, state, location,
  settings}}, "service": ...}`. The single-character shape remains available
  until the dashboard is updated.
- The touch page shows one card per character with the existing controls.

## Template

One additional `video` widget per character in `lcd_templates.json`, each
with its own `state_file`/`position_file` and that character's animation
variants. Widget order defines z-order (later widgets draw on top).

## Art requirements per character

The full Patch set is 27 animations, but a new character can ship playable
with a subset (~14): idle, blink, one or two micro-idles, active, wait,
enter, exit, walk left/right, plus its specialty states (e.g. `doctor`:
thermal-warm/hot/critical/relief). Missing selector states fall back to the
widget's base `path` clip, so partial sets degrade gracefully.

## Migration

1. Ship the roster-aware watcher writing both v1 (un-suffixed) and v2
   (suffixed) files for `patch`. Nothing visible changes.
2. Add the new character's assets and template widget; enable its roster
   entry.
3. Migrate the dashboard to the multi-character snapshot.
4. Disable v1 compatibility output once the template no longer references
   un-suffixed files.

Each step is independently revertible.

## Measured performance context (July 2026, host y70)

The renderer daemon runs at ~27 % of one core, dominated by the full-canvas
24 fps background video recomposite plus live libx264 encode of the
2288×1048 stream; the watcher costs 0.2 % CPU / <1 MB. An additional
600×400 character at 10 fps adds one PNG frame decode and blit per frame
into a canvas that is already being re-encoded — about 1–3 % of a core and,
without a shared clip cache, roughly the encoded size of its animation set
in RSS.

## Sizing

| work                                   | size |
|----------------------------------------|------|
| roster + per-character engines/outputs | L    |
| command v2 field + bridge/API/UI       | M    |
| separation policy + tests              | M    |
| template widget + install rendering    | S    |
| renderer shared clip cache (optional)  | M    |
