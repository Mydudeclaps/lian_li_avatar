# Architecture

```text
Codex / Claude hooks ──> lianli-agent-event ─┐
                                             │
/proc + cgroup activity ─────────────────────┤
CPU / GPU / coolant temperatures ────────────┼─> lianli-agent-watch
Grand Line control bridge ───────────────────┘        │
                                                      ├─ state file
                                                      ├─ position file
                                                      └─ control status
                                                               │
                                  patched lian-li-linux renderer ┘
```

The components communicate through short versioned records in the current
user's trusted runtime directory. The watcher is the only writer of visible
state and position. The web bridge can only publish allowlisted control
commands and preferences.

State priority is deterministic: critical thermals, error/attention events,
explicit completion, enter/exit transitions, tool/read activity, then ambient
micro-idles and roaming. Any real activity interrupts ambient movement.

Position records use the template's unrotated 2288×1048 logical coordinate
system:

```text
v1 <sequence> <center-x> <center-y> <duration-ms>
```

The renderer interpolates both axes before applying the physical panel's
orientation. That is why left/right movement remains correct on a rotated
HydroShift II Curved display.

Event records contain no prompt or tool payload:

```text
v1 <created-unix-ms> <start|exit|tool|read|permission|error|success> <ttl-ms>
```

The watcher drives a roster of characters (`patch` plus any crewmates
enabled in the `characters` config list), each with its own state engine and
character-suffixed output files (`lianli-agent-state-<id>` and friends).
Events route to every character whose actor mask includes the emitting
agent; commands address one character via the optional v2 field
(`v1 <ms> <character|-> <command> <argument>`), with four-field v1 records
resolving to `patch`, which also keeps writing the un-suffixed v1 files
until templates are migrated. Characters keep apart through distinct home
anchors and a roam reservation: no roam starts toward a waypoint within
500 px of another character's resting point or walk destination (one
re-roll, then the ambient slot is skipped); mid-flight crossings are
allowed.

Event and control records are published as one file per record —
`<name>.<20-digit zero-padded created-ms>.<pid>` — so bursts inside one
100 ms watcher tick cannot overwrite each other. Publication is
no-clobber (link, not rename), and the watcher merges claimed records by
embedded created-ms before applying them; records sharing one millisecond
have no defined relative order. The watcher claims each record atomically by
rename, always deletes what it claims, and sweeps abandoned claims and stale
temporaries at startup. Queues are bounded per writer — writers drop the
oldest records beyond 32 per actor and the control bridge rejects new
commands beyond 16 pending — though simultaneous writers can briefly
overshoot a bound by their count. The watcher also still consumes the legacy
single-mailbox file names during upgrades.

See the inline Rust and Python tests for strict parsing, ownership checks,
stale-record rejection, animation priorities, roaming behavior, and the
control API contract.
