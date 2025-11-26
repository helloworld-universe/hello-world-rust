- spawn plugins as separate processes and manage timeouts (cross-platform),
- migrate the host to Wasmtime and show `ResourceLimiter` usage,
- implement host-provided import APIs (e.g., `host::log`) and an example plugin using them,
- make hot-reload only reload changed plugin(s).

- track live child processes in the `PluginManager` so hot-reload restarts only affected plugins (no duplicates),
- add per-plugin resource limits using OS facilities (cgroups on Linux),
- add a small supervisor that restarts crashing plugins with backoff, and health checks.
