# Security and privacy

- Hook commands discard stdin. Prompts, tool arguments, terminal text, and
  model output are neither read nor stored.
- Runtime event, state, command, and preference files are atomically written
  with private permissions and strict size/value allowlists.
- The control server binds to loopback, exposes only four static files and
  three API routes, rejects cross-origin mutations, and caps JSON bodies at
  16 KiB.
- Live Codex/Claude settings, Lian Li device configuration, hardware serials,
  runtime files, binaries, credentials, caches, and backups are excluded.
- The full local Y70 dashboard is not bundled. Its unrelated server had a
  static-file traversal flaw; this focused bridge uses exact route mappings
  and includes raw and percent-encoded traversal regression tests.

The generated hook files are examples. Merge them into existing settings
instead of replacing whole files. Codex also requires explicit `/hooks`
review after its rendered hook configuration changes.
