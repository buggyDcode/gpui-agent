//! Shared harness for the headless-todo integration tests. Each test
//! binary gets its own copy — `dead_code` is allowed because not every
//! binary uses every helper.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gpui_agent::AgentHost;
use gpui_agent::protocol::PlatformKind;
use gpui_agent::server::spawn_host;
use serde_json::json;
use todo_core::TodoStore;

/// A spawned headless todo host; `token` doubles as the HMAC secret and
/// a per-test-file marker in the spawned process's args.
pub struct Todo {
    pub addr: String,
    shutdown: Arc<AtomicBool>,
    store: Arc<Mutex<TodoStore>>,
}

pub struct RuntimeRoot {
    path: PathBuf,
}

impl RuntimeRoot {
    pub fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gpui-agent-cli-runtime-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create isolated runtime root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("make isolated runtime root private");
        }
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RuntimeRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl Todo {
    pub fn spawn(token: &str) -> Self {
        let store = Arc::new(Mutex::new(TodoStore::new(PlatformKind::Headless)));
        let (addr, shutdown) = spawn_host(
            "127.0.0.1:0".parse().unwrap(),
            Some(token.to_owned()),
            store.clone(),
        )
        .expect("bind");
        Self {
            addr: addr.to_string(),
            shutdown,
            store,
        }
    }

    pub fn seed(&self, titles: &[&str]) {
        let mut store = self.store.lock().unwrap();
        for title in titles {
            store
                .dispatch(&gpui_agent::Op::Invoke {
                    name: "todo.add".into(),
                    args: json!({ "title": title }),
                })
                .expect("seed add");
        }
    }

    pub fn titles(&self) -> Vec<String> {
        self.store
            .lock()
            .unwrap()
            .items()
            .iter()
            .map(|t| t.title.clone())
            .collect()
    }

    pub fn draft(&self) -> String {
        self.store.lock().unwrap().draft().to_owned()
    }

    pub fn done_titles(&self) -> Vec<String> {
        self.store
            .lock()
            .unwrap()
            .items()
            .iter()
            .filter(|t| t.done)
            .map(|t| t.title.clone())
            .collect()
    }
}

impl Drop for Todo {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}
