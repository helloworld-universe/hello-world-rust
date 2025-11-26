use super::{PluginManager, Event};
use std::{sync::Arc, error::Error};

pub async fn plugin_main() -> Result<(), Box<dyn Error>> {
    let plugins_dir = std::env::current_dir()?.join("plugins");
    let manager = Arc::new(PluginManager::new());
    manager.load_all(&plugins_dir).await?;

    let manager_clone = manager.clone();
    tokio::spawn(async move {
        manager_clone.fire_event(Event {
            topic: "hello_world".into(),
            payload: serde_json::json!({ "msg": "Hello from host!" }),
        }).await;

        manager_clone.fire_event(Event {
            topic: "boot".into(),
            payload: serde_json::json!({ "msg": "Begin boot sequence" }),
        }).await;
    });

    // Example: periodic event emission from host
    let manager_clone = manager.clone();
    tokio::spawn(async move {
        let mut i: u64 = 0;
        loop {
            let ev = Event { topic: "tick".to_string(), payload: serde_json::json!({"count": i}) };
            manager_clone.fire_event(ev).await;
            i += 1;
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });

    // keep main alive
    loop { tokio::time::sleep(std::time::Duration::from_secs(3600)).await; }
}
