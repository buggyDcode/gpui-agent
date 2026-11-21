//! Shared harness for the headless-todo integration tests. Each test
//! binary gets its own copy — `dead_code` is allowed because not every
//! binary uses every helper.
#![expect(dead_code, reason = "each test binary uses a subset of helpers")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gpui_agent::AgentHost;
use gpui_agent::protocol::PlatformKind;
use gpui_agent::server::spawn_host;
use serde_json::json;
use todo_core::TodoStore;

/// A spawned headless todo host; `token` doubles as the HMAC secret and
/// a per-test-file marker in the spawned process's args.
pub(crate) struct Todo {
    pub addr: String,
    shutdown: Arc<AtomicBool>,
    store: Arc<Mutex<TodoStore>>,
}

pub(crate) struct RuntimeRoot {
    path: PathBuf,
}

static NEXT_RUNTIME_ROOT_ID: AtomicU64 = AtomicU64::new(0);

impl RuntimeRoot {
    pub(crate) fn new() -> Self {
        let path = loop {
            let id = NEXT_RUNTIME_ROOT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "gpui-agent-cli-runtime-{}-{id}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => break path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create isolated runtime root: {error}"),
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("make isolated runtime root private");
        }
        Self { path }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for RuntimeRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl Todo {
    pub(crate) fn spawn(token: &str) -> Self {
        let store = Arc::new(Mutex::new(TodoStore::new(PlatformKind::Headless)));
        let (addr, shutdown) = spawn_host(
            "127.0.0.1:0".parse().expect("test fixture"),
            Some(token.to_owned()),
            std::sync::Arc::clone(&store),
        )
        .expect("bind");
        Self {
            addr: addr.to_string(),
            shutdown,
            store,
        }
    }

    pub(crate) fn seed(&self, titles: &[&str]) {
        let mut store = self.store.lock().expect("test fixture");
        for title in titles {
            store
                .dispatch(&gpui_agent::Op::Invoke {
                    name: "todo.add".into(),
                    args: json!({ "title": title }),
                })
                .expect("seed add");
        }
    }

    pub(crate) fn titles(&self) -> Vec<String> {
        self.store
            .lock()
            .expect("test fixture")
            .items()
            .iter()
            .map(|t| t.title.clone())
            .collect()
    }

    pub(crate) fn draft(&self) -> String {
        self.store.lock().expect("test fixture").draft().to_owned()
    }

    pub(crate) fn done_titles(&self) -> Vec<String> {
        self.store
            .lock()
            .expect("test fixture")
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
