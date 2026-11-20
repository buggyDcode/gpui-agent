mod common;

use std::process::{Command, Output};

use common::{RuntimeRoot, Todo};

const TOKEN: &str = "runtime-root-test-secret";

fn cli(runtime: &RuntimeRoot, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(args)
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .env_remove("GPUI_AGENT_SESSION")
        .env_remove("GPUI_AGENT_ADDR")
        .env_remove("GPUI_AGENT_CONNECT")
        .env_remove("GPUI_AGENT_TOKEN")
        .env_remove("GPUI_AGENT_ALLOW_REMOTE")
        .output()
        .expect("spawn gpui-agent")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[cfg(unix)]
#[test]
fn snapshot_refs_tightens_legacy_runtime_root_before_attach() {
    use std::os::unix::fs::PermissionsExt;

    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    let snapshot = cli(
        &runtime,
        &[
            "--token",
            TOKEN,
            "--addr",
            &todo.addr,
            "snapshot",
            "--refs",
        ],
    );
    assert!(
        snapshot.status.success(),
        "snapshot --refs failed: {}",
        stderr(&snapshot)
    );
    assert_eq!(
        std::fs::metadata(runtime.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let attached = cli(
        &runtime,
        &["--session", "app", "--token", TOKEN, "attach", &todo.addr],
    );
    assert!(
        attached.status.success(),
        "attach after snapshot --refs failed: {}",
        stderr(&attached)
    );
}
