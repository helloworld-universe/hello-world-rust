use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::{Path, PathBuf}, sync::Arc};
use tokio::sync::Mutex;
use wasmtime::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};
use tracing::{info, error};

pub mod boot;
pub use boot::plugin_main;

/// Event exchanged between host and plugins.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Event {
    pub topic: String,
    pub payload: serde_json::Value,
}

/// Plugin manifest file (plugin.json).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginManifest {
    pub name: String,
    pub version: Option<String>,
    /// list of topic strings this plugin wants to subscribe to.
    pub subscriptions: Vec<String>,
    /// optional names for functions exported by plugin
    pub entry: Option<String>,
    pub alloc: Option<String>,
}

/// A loaded plugin and its runtime instance(s).
#[allow(dead_code)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: PluginManifest,
    engine: Engine,
    module_path: PathBuf,
    module: Module,

    // mutable instance that we can replace on reload
    inner: Arc<Mutex<PluginInstance>>,
}

#[allow(dead_code)]
pub struct PluginInstance {
    store: Store<()>,
    instance: Instance,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    handle_event: TypedFunc<(i32, i32), ()>,
}

impl Plugin {
    /// Load plugin from plugin directory (must contain plugin.json and a wasm file)
    pub async fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let manifest_path = dir.join("plugin.json");
        let manifest_text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("reading manifest {:?}", manifest_path))?;
        let manifest: PluginManifest = serde_json::from_str(&manifest_text)
            .context("parsing plugin.json")?;

        // find a wasm file: prefer plugin.wasm, else first *.wasm
        let wasm_path = {
            let p = dir.join("plugin.wasm");
            if p.exists() {
                p
            } else {
                let mut found = None;
                for e in fs::read_dir(&dir)? {
                    let e = e?;
                    let p = e.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("wasm") {
                        found = Some(p);
                        break;
                    }
                }
                found.context("no wasm file in plugin dir")?
            }
        };

        let engine = Engine::default();
        let module = Module::from_file(&engine, &wasm_path)
            .with_context(|| format!("loading wasm from {:?}", wasm_path))?;

        // create initial instance
        let inner = Self::instantiate(&engine, &module, manifest.clone()).context("instantiating plugin")?;

        Ok(Self {
            name: manifest.name.clone(),
            dir,
            manifest,
            engine,
            module_path: wasm_path,
            module,
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    fn instantiate(engine: &Engine, module: &Module, manifest: PluginManifest) -> Result<PluginInstance> {
        let mut store = Store::new(engine, ());
        let mut linker = Linker::new(engine);

        // -------------------------------------------------------
        // Host function: host_log(ptr: *const u8, len: usize)
        // -------------------------------------------------------
        linker.func_wrap(
            "host",
            "host_log",
            |mut caller: wasmtime::Caller<'_, ()>, ptr: i32, len: i32| {
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("plugin must export memory");

                let mut buf = vec![0u8; len as usize];
                memory
                    .read(&caller, ptr as usize, &mut buf)
                    .expect("memory read failed");

                let s = String::from_utf8_lossy(&buf);
                tracing::info!("[plugin] {}", s);
            },
        )?;

        // Add host functions here if you want, e.g. host_log. For simplicity we don't add imports now.
        let instance = linker.instantiate(&mut store, module).context("wasm instantiate")?;
        let memory = instance.get_memory(&mut store, "memory").context("export memory required")?;

        let alloc_name = manifest.alloc.clone().unwrap_or_else(|| "alloc".to_string());
        let alloc = instance.get_typed_func::<i32, i32>(&mut store, &alloc_name)
            .with_context(|| format!("alloc function `{}` missing", alloc_name))?;

        let entry_name = manifest.entry.clone().unwrap_or_else(|| "handle_event".to_string());
        let handle_event = instance.get_typed_func::<(i32, i32), ()>(&mut store, &entry_name)
            .with_context(|| format!("entry function `{}` missing", entry_name))?;

        Ok(PluginInstance { store, instance, memory, alloc, handle_event })
    }

    /// Send event to the running instance (serialize to json and copy into plugin memory)
    pub async fn send_event(&self, ev: &Event) -> Result<()> {
        let payload = serde_json::to_vec(ev)?;
        let mut inst = self.inner.lock().await;
        let size = payload.len() as i32;

        // FIX 1: Remove the '&'. Copy the handle instead of borrowing it.
        let alloc_func = inst.alloc.clone();

        // Now `inst` is not borrowed, so we can mutably borrow `inst.store` here
        let ptr = alloc_func.call(&mut inst.store, size).context("alloc call failed")?;

        // write into memory
        let memory_inst = inst.memory;
        let data = memory_inst.data_mut(&mut inst.store);

        let start = ptr as usize;
        let end = start + payload.len();

        if end > data.len() {
            anyhow::bail!("plugin memory insufficient: need {} bytes (mem_len={})", end, data.len());
        }

        data[start..end].copy_from_slice(&payload);

        // FIX 2: Remove the '&'. Copy the handle here as well.
        let handle_event_func = inst.handle_event.clone();

        handle_event_func.call(&mut inst.store, (ptr, size)).context("handle_event failed")?;

        Ok(())
    }

    /// Reloads the wasm module (recreate instance) and swap in place. If reload fails, keep previous instance.
    #[allow(dead_code)]
    pub async fn reload(&self) -> Result<()> {
        info!("Reloading plugin {}", self.name);
        let module = Module::from_file(&self.engine, &self.module_path)
            .with_context(|| format!("loading wasm during reload from {:?}", self.module_path))?;
        let new_inst = Self::instantiate(&self.engine, &module, self.manifest.clone())
            .context("instantiate on reload failed")?;
        let mut guard = self.inner.lock().await;
        *guard = new_inst;
        info!("Reloaded plugin {}", self.name);
        Ok(())
    }
}

/// Manager holds loaded plugins and topic subscriptions.
pub struct PluginManager {
    plugins: Arc<Mutex<HashMap<String, Arc<Plugin>>>>,
    topics: Arc<Mutex<HashMap<String, Vec<Arc<Plugin>>>>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(Mutex::new(HashMap::new())),
            topics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Load all plugin directories found under `plugins_dir`.
    pub async fn load_all(&self, plugins_dir: impl AsRef<Path>) -> Result<()> {
        let plugins_dir = plugins_dir.as_ref();
        if !plugins_dir.exists() { return Ok(()); }
        for ent in fs::read_dir(plugins_dir)? {
            let ent = ent?;
            let path = ent.path();
            if path.is_dir() {
                if let Err(e) = self.load_plugin(path.clone()).await {
                    error!("Failed to load plugin {:?}: {:?}", path, e);
                }
            }
        }
        Ok(())
    }

    /// Load a single plugin dir and register subscriptions.
    pub async fn load_plugin(&self, dir: PathBuf) -> Result<()> {
        let plugin = Plugin::load_from_dir(&dir).await?;
        let name = plugin.name.clone();
        let arc = Arc::new(plugin);

        // register in plugins map
        self.plugins.lock().await.insert(name.clone(), arc.clone());

        // register subscriptions
        let mut topics = self.topics.lock().await;
        for topic in arc.manifest.subscriptions.iter() {
            topics.entry(topic.clone()).or_default().push(arc.clone());
        }

        info!("Loaded plugin {} with subscriptions: {:?}", name, arc.manifest.subscriptions);
        Ok(())
    }

    /// Fire an event: dispatch to all subscribers of `event.topic`.
    pub async fn fire_event(&self, ev: Event) {
        let subs = {
            let topics = self.topics.lock().await;
            topics.get(&ev.topic).cloned()
        };

        if let Some(list) = subs {
            for plugin in list.into_iter() {
                let ev = ev.clone();
                let plugin = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = plugin.send_event(&ev).await {
                        error!("Sending event to plugin {} failed: {:?}", plugin.name, e);
                    }
                });
            }
        }
    }
}
