mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{RuntimeRoot, Todo};

const TOKEN: &str = "runtime-root-test-secret";

fn cli(runtime: &RuntimeRoot, args: &[&str]) -> Output {
    cli_with_root(runtime.path(), args)
}

fn cli_with_root(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(args)
        .env("GPUI_AGENT_RUNTIME_DIR", root)
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(unix)]
#[test]
fn unsafe_runtime_root_errors_reach_discovery_commands() {
    let runtime = RuntimeRoot::new();
    let target = RuntimeRoot::new();
    let unsafe_root = runtime.path().join("runtime-link");
    std::os::unix::fs::symlink(target.path(), &unsafe_root).unwrap();

    let targets = cli_with_root(&unsafe_root, &["targets"]);
    assert!(!targets.status.success());
    assert!(
        stderr(&targets).contains("unsafe runtime directory"),
        "{}",
        stderr(&targets)
    );

    let doctor = cli_with_root(&unsafe_root, &["doctor"]);
    assert!(!doctor.status.success());
    assert!(
        stdout(&doctor).contains("fail  discovery: could not use runtime directory"),
        "{}",
        stdout(&doctor)
    );

    let connect = cli_with_root(&unsafe_root, &["--connect", "fake-app", "hello"]);
    assert!(!connect.status.success());
    assert!(
        stderr(&connect).contains("unsafe runtime directory"),
        "{}",
        stderr(&connect)
    );
    assert!(!stderr(&connect).contains("no AgentHost"));

    std::fs::remove_file(&unsafe_root).unwrap();
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
        &["--token", TOKEN, "--addr", &todo.addr, "snapshot", "--refs"],
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
