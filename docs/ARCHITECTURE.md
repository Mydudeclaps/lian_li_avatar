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

See the inline Rust and Python tests for strict parsing, ownership checks,
stale-record rejection, animation priorities, roaming behavior, and the
control API contract.
