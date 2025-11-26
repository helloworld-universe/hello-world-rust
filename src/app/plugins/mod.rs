/*
# WASM Plugin Framework

use anyhow::Result;
use std::{fs, path::Path};
use wasmer::{Instance, Module, Store};
use wasmer_wasi::{WasiEnv, WasiState};

pub fn load_plugins() -> Result<()> {
    let plugins_dir = Path::new("plugins");
    tracing::info!("Scanning `{}` for plugins…", plugins_dir.display());

    if !plugins_dir.exists() {
        tracing::debug!("No plugins directory found. Creating '{}'…", plugins_dir.display());
        fs::create_dir_all(plugins_dir)?;
        tracing::debug!("Place .wasm plugin files in '{}' and re-run.", plugins_dir.display());

        return Ok(());
    }

    let store = Store::default();

    for entry in fs::read_dir(plugins_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("wasm") {
            continue;
        }

        tracing::debug!("=== Loading plugin: {} ===\n", path.display());
        let wasm_bytes = fs::read(&path)?;

        let module = Module::new(&store, &wasm_bytes)?;
        let mut wasi_env = WasiState::new("plugin")
            .inherit_stdout()
            .inherit_stderr()
            .finalize()?;

        let import_object = wasi_env.import_object(&module)?;
        let instance = Instance::new(&module, &import_object)?;

        if let Ok(start) = instance.exports.get_function("_start") {
            tracing::debug!("Running plugin {}…", path.display());
            start.call(&[])?;
        } else {
            tracing::debug!("Plugin {} has no _start export. Skipping.", path.display());
        }
    }

    tracing::info!("All plugins finished.");
    Ok(())
}
*/

/*

Workspace with three components:
- `host` — supervisor/manager that scans `plugins/`, spawns per-plugin `runner` processes, tracks children, monitors resource usage, hot-reloads only changed plugins, and implements a simple event bus and backoff supervisor.
- `runner` — simple process that loads a single `.wasm` plugin with Wasmtime (in-process), provides host imports (`host::log`, `host::register_event`), listens on stdin for host commands (e.g. `FIRE:event_name`) and writes registration messages to stdout that the host parses.
- `plugin` — example plugin that registers for `startup` and implements `on_event`.

This design is portable (works on FreeBSD/macOS/Linux/Windows). For process resource monitoring we use the `sysinfo` crate (cross-platform). For POSIX rlimits we optionally set them in the `runner` (cfg(unix)).

*/

/*
## Workspace layout

```
wasm-plugin-recommended/
├─ Cargo.toml
├─ host/
│  ├─ Cargo.toml
│  └─ src/main.rs
├─ runner/
│  ├─ Cargo.toml
│  └─ src/main.rs
└─ plugin/
   ├─ Cargo.toml
   └─ src/lib.rs

---

## Top-level Cargo.toml

```toml
[workspace]
members = ["host", "runner", "plugin"]
```
*/

/*
## host/Cargo.toml

```toml
[package]
name = "host"
version = "0.1.0"
edition = "2021"

[dependencies]
notify = "6"
sysinfo = { version = "0.28", features = ["process", "system"] }
simple_logger = "1.16"
log = "0.4"
serde = { version = "1.0", features = ["derive"] }
anyhow = "1"
backoff = "0.4"
*/

/*

```toml
[package]
name = "runner"
version = "0.1.0"
edition = "2021"

[dependencies]
wasmtime = { version = "8", features = ["wasi"] }
wasmtime-wasi = "8"
simple_logger = "1.16"
log = "0.4"
serde = { version = "1.0", features = ["derive"] }
anyhow = "1"

[features]
default = []
```
*/

/*
## plugin/Cargo.toml

```toml
[package]
name = "plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
```
*/

/*

```rust
use anyhow::Result;
use backoff::ExponentialBackoff;
use log::{info, warn, error};
use notify::{Watcher, RecursiveMode, watcher, DebouncedEvent};
use serde::Deserialize;
use simple_logger::SimpleLogger;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{ProcessExt, SystemExt, System};

#[derive(Debug, Deserialize, Clone, Default)]
struct Manifest {
    name: Option<String>,
    runtime: Option<String>,
    timeout_ms: Option<u64>,
    restart_backoff_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct PluginSpec { name: String, wasm: PathBuf, manifest: Manifest }

#[derive(Debug)]
struct ManagedChild {
    child: Child,
    started: Instant,
    last_heartbeat: Instant,
    restarts: u32,
}

type ChildMap = Arc<Mutex<HashMap<String, ManagedChild>>>;
type EventRegistry = Arc<Mutex<HashMap<String, HashSet<String>>>>;

fn read_manifest(wasm: &Path) -> Manifest {
    let p = wasm.with_extension("json");
    if p.exists() {
        match fs::read_to_string(&p).and_then(|s| serde_json::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))) {
            Ok(m) => m,
            Err(e) => { warn!("Failed to read manifest {}: {}", p.display(), e); Manifest::default() }
        }
    } else { Manifest::default() }
}

fn scan_plugins(dir: &Path) -> Result<Vec<PluginSpec>> {
    let mut out = vec![];
    for e in fs::read_dir(dir)? {
        let e = e?;
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) == Some("wasm") {
            let manifest = read_manifest(&path);
            let name = manifest.name.clone().unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into_owned());
            out.push(PluginSpec { name, wasm: path, manifest });
        }
    }
    Ok(out)
}

fn spawn_runner(spec: &PluginSpec, children: &ChildMap, event_registry: &EventRegistry) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir("runner")
        .arg("run")
        .arg("--release")
        .arg("--")
        .arg(spec.wasm.to_string_lossy().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // spawn
    let mut child = cmd.spawn()?;
    let pid = child.id();
    info!("Spawned runner for {} pid={}", spec.name, pid);

    // stdout reader to parse registration messages
    if let Some(out) = child.stdout.take() {
        let name = spec.name.clone();
        let reg = event_registry.clone();
        thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().flatten() {
                // protocol: lines starting with "REGISTER:" indicate plugin registration
                if let Some(ev) = line.strip_prefix("REGISTER:") {
                    info!("Plugin {} registered for event {}", name, ev);
                    let mut r = reg.lock().unwrap();
                    r.entry(ev.to_string()).or_default().insert(name.clone());
                } else if let Some(hb) = line.strip_prefix("HEARTBEAT") {
                    // optional heartbeat handling
                    info!("hb {}: {}", name, hb);
                } else {
                    info!("[plugin:{}] {}", name, line);
                }
            }
        });
    }

    // stderr reader to log
    if let Some(err) = child.stderr.take() {
        let name = spec.name.clone();
        thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().flatten() {
                warn!("[plugin:{} stderr] {}", name, line);
            }
        });
    }

    let managed = ManagedChild { child, started: Instant::now(), last_heartbeat: Instant::now(), restarts: 0 };
    children.lock().unwrap().insert(spec.name.clone(), managed);
    Ok(())
}

fn send_fire_command(children: &ChildMap, name: &str, event: &str) -> Result<()> {
    let mut guard = children.lock().unwrap();
    if let Some(m) = guard.get_mut(name) {
        if let Some(stdin) = m.child.stdin.as_mut() {
            writeln!(stdin, "FIRE:{}", event)?;
            stdin.flush()?;
        }
    }
    Ok(())
}

fn monitor_children(children: &ChildMap, plugin_dir: &Path) {
    let poll_interval = Duration::from_millis(500);
    let sys = Arc::new(Mutex::new(System::new_all()));
    loop {
        thread::sleep(poll_interval);
        let mut to_remove = Vec::new();
        {
            let mut s = sys.lock().unwrap();
            s.refresh_processes();
            let guard = children.lock().unwrap();
            for (name, managed) in guard.iter() {
                let pid = managed.child.id() as i32;
                if let Some(p) = s.process(pid) {
                    let mem = p.memory(); // kilobytes
                    let cpu = p.cpu_usage();
                    if mem > 200_000 { // e.g., 200 MB RSS
                        warn!("Killing {} for excessive memory: {} KB", name, mem);
                        let _ = managed.child.kill();
                    }
                    if cpu > 90.0 {
                        warn!("High CPU {} -> {}%", name, cpu);
                    }
                }
            }
        }

        // reap exited children
        {
            let mut guard = children.lock().unwrap();
            let keys: Vec<String> = guard.keys().cloned().collect();
            for k in keys {
                if let Some(m) = guard.get_mut(&k) {
                    match m.child.try_wait() {
                        Ok(Some(status)) => {
                            info!("Child {} exited with {:?}", k, status);
                            to_remove.push(k.clone());
                        }
                        Ok(None) => {}
                        Err(e) => { warn!("try_wait error {}: {:?}", k, e); to_remove.push(k.clone()); }
                    }
                }
            }
            for k in to_remove.iter() { guard.remove(k); }
        }
    }
}

fn main() -> Result<()> {
    SimpleLogger::new().init().unwrap();
    let plugins_dir = PathBuf::from("plugins");
    fs::create_dir_all(&plugins_dir)?;

    let children: ChildMap = Arc::new(Mutex::new(HashMap::new()));
    let event_registry: EventRegistry = Arc::new(Mutex::new(HashMap::new()));

    // initial scan
    let specs = scan_plugins(&plugins_dir)?;
    for spec in specs.iter() { spawn_runner(spec, &children, &event_registry)?; }

    // start monitor thread
    let children_mon = children.clone();
    let pd = plugins_dir.clone();
    thread::spawn(move || monitor_children(&children_mon, &pd));

    // watcher for hot-reload — only reload changed plugins
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = watcher(tx, Duration::from_millis(500))?;
    watcher.watch(&plugins_dir, RecursiveMode::NonRecursive)?;
    loop {
        match rx.recv() {
            Ok(DebouncedEvent::Write(p)) | Ok(DebouncedEvent::Create(p)) | Ok(DebouncedEvent::Remove(p)) | Ok(DebouncedEvent::Rename(_, p)) => {
                if p.extension().and_then(|s| s.to_str()) == Some("wasm") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        let name = stem.to_string();
                        info!("Hot-reload: detected change for {}", name);
                        // kill old
                        if let Some(mut m) = children.lock().unwrap().remove(&name) {
                            let _ = m.child.kill();
                            let _ = m.child.wait();
                        }
                        // restart
                        let spec = PluginSpec { name: name.clone(), wasm: p.clone(), manifest: read_manifest(&p) };
                        let _ = spawn_runner(&spec, &children, &event_registry);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => { error!("watch error: {:?}", e); break; }
        }
    }
}
*/

/*
## runner/src/main.rs

This binary loads one wasm file (path from argv[1]), instantiates with Wasmtime, implements host imports, and listens on stdin for `FIRE:<event>` to call `on_event`.

```rust
use anyhow::Result;
use log::{info, warn};
use simple_logger::SimpleLogger;
use std::env;
use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};
use wasmtime::*;
use wasmtime_wasi::WasiCtxBuilder;

#[cfg(unix)]
fn apply_posix_rlimits() {
    // Example: limit address space to 256MB and CPU to 30s
    unsafe {
        use libc::{rlimit, RLIMIT_AS, RLIMIT_CPU, setrlimit};
        let mem = rlimit { rlim_cur: 256 * 1024 * 1024, rlim_max: 256 * 1024 * 1024 };
        let cpu = rlimit { rlim_cur: 30, rlim_max: 60 };
        let _ = setrlimit(RLIMIT_AS, &mem);
        let _ = setrlimit(RLIMIT_CPU, &cpu);
    }
}

fn main() -> Result<()> {
    SimpleLogger::new().init().unwrap();
    let args: Vec<String> = env::args().collect();
    let wasm = args.get(1).expect("wasm path");

    #[cfg(unix)]
    apply_posix_rlimits();

    // wasmtime engine
    let mut config = Config::new();
    config.consume_fuel(true);
    config.static_memory_maximum_size(64 * 1024 * 1024); // 64MB
    let engine = Engine::new(&config)?;
    let mut store = Store::new(&engine, ());
    store.add_fuel(10_000_000)?; // fuel budget

    let module = Module::from_file(&engine, wasm)?;
    let mut linker = Linker::new(&engine);

    // wasi
    let wasi = WasiCtxBuilder::new().inherit_stdio().build();
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;

    // host::log(ptr,len)
    linker.func_wrap("host", "log", move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
            let data = mem.data(&caller);
            let start = ptr as usize; let end = start + len as usize;
            if end <= data.len() {
                if let Ok(s) = std::str::from_utf8(&data[start..end]) {
                    println!("PLUGIN_LOG:{}", s);
                }
            }
        }
    })?;

    // host::register_event(ptr,len) -> writes REGISTER:<event> to stdout for host parsing
    linker.func_wrap("host", "register_event", move |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
        if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
            let data = mem.data(&caller);
            let start = ptr as usize; let end = start + len as usize;
            if end <= data.len() {
                if let Ok(s) = std::str::from_utf8(&data[start..end]) {
                    println!("REGISTER:{}", s);
                }
            }
        }
    })?;

    let mut store_data = (); // placeholder
    let mut store = Store::new(&engine, store_data);

    let instance = linker.instantiate(&mut store, &module)?;

    // call _start if present
    if let Ok(start) = instance.get_func(&mut store, "_start") {
        let _ = start.call(&mut store, &[], &mut []);
    }

    // spawn thread to read stdin for FIRE:<event>
    let inst = instance.clone();
    let mut store_clone = store.clone();
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines().flatten() {
            if let Some(ev) = line.strip_prefix("FIRE:") {
                // call on_event if exported
                if let Ok(f) = inst.get_func(&mut store_clone, "on_event") {
                    let _ = f.call(&mut store_clone, &[], &mut []);
                    println!("HANDLED_EVENT:{}", ev);
                }
            }
        }
    });

    // keep process alive while wasm runs — simplistic: wait on stdin EOF
    let stdin = io::stdin();
    for _ in stdin.lock().lines() { }

    Ok(())
}
*/

/*
Note: include `use std::thread;` at top of runner.

---

## plugin/src/lib.rs

```rust
#[no_mangle]
pub extern "C" fn _start() {
    // call host::register_event("startup")
    let s = "startup";
    unsafe { host_register_event(s.as_ptr() as *const u8, s.len()); }
}

extern "C" {
    fn host_register_event(ptr: *const u8, len: usize);
    fn host_log(ptr: *const u8, len: usize);
}

#[no_mangle]
pub extern "C" fn on_event() {
    let msg = "plugin: got event";
    unsafe { host_log(msg.as_ptr() as *const u8, msg.len()); }
}
*/

/*
Build plugin with `wasm32-unknown-unknown` or `wasm32-wasi` depending on your toolchain. For the example above, using `wasm32-unknown-unknown` and a simple linker like `wasm-bindgen` is fine; but in practice `wasm32-wasi` works well with wasmtime and wasi imports.

---

## How it works (summary)

- `host` scans `plugins/` and spawns `runner` for each `.wasm` file. The runner runs the Wasm plugin inside Wasmtime with resource limits (fuel + memory cap). The runner implements host imports `host::register_event` and `host::log` by writing structured lines to stdout.
- The host parses runner stdout to build an event registry mapping event names to plugin names.
- To fire an event, the host writes `FIRE:<event>` to the runner's stdin; the runner calls the plugin's `on_event` export.
- Hot-reload: `notify` watches `plugins/`. When a single `.wasm` changes, the host kills and restarts only that plugin's runner.
- Supervisor/backoff: host can detect child exit and restart with exponential backoff using `backoff` crate (left for extension but scaffolding present).
- Soft resource monitor: host uses `sysinfo` to poll child RSS and CPU and kills the child if thresholds are exceeded (portable). Runner also sets POSIX rlimits on Unix platforms.

---

## Next steps

I can now:
- commit these files into the canvas as runnable sources (I created this document but haven't injected code into the existing host/runner/plugin canvases),
- produce a ZIP you can download,
- wire full restart/backoff details and health-check endpoints,
- tweak plugin memory/fuel defaults.

*/
