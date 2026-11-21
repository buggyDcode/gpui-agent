//! Per-run isolated runtime roots: creation and collision handling.

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

fn cli_with_root_and_path(root: &Path, args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(args)
        .env("PATH", path)
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
    use std::os::unix::fs::PermissionsExt;

    let runtime = RuntimeRoot::new();
    let target = RuntimeRoot::new();
    let unsafe_root = runtime.path().join("runtime-link");
    std::os::unix::fs::symlink(target.path(), &unsafe_root).expect("test fixture");

    let adb_dir = RuntimeRoot::new();
    let adb = adb_dir.path().join("adb");
    std::fs::write(
        &adb,
        "#!/bin/sh\ncase \"$1\" in\n  devices) printf 'List of devices attached\\nemulator-5554\\tdevice\\n' ;;\n  forward) printf 'emulator-5554 tcp:17421 tcp:17421\\n' ;;\nesac\n",
    )
    .expect("test fixture");
    std::fs::set_permissions(&adb, std::fs::Permissions::from_mode(0o700)).expect("test fixture");

    let targets = cli_with_root_and_path(&unsafe_root, &["targets"], adb_dir.path());
    assert!(!targets.status.success());
    assert!(
        stderr(&targets).contains("unsafe runtime directory"),
        "{}",
        stderr(&targets)
    );
    let targets: serde_json::Value = serde_json::from_str(&stdout(&targets)).expect("test fixture");
    assert_eq!(targets["adb"][0]["serial"], "emulator-5554");
    assert_eq!(targets["adb"][0]["forwarded"][0], "127.0.0.1:17421");

    let sessions = cli_with_root(&unsafe_root, &["sessions"]);
    assert!(!sessions.status.success());
    assert!(
        stderr(&sessions).contains("unsafe runtime directory"),
        "{}",
        stderr(&sessions)
    );

    let doctor = cli_with_root(&unsafe_root, &["doctor"]);
    assert!(!doctor.status.success());
    assert!(
        stdout(&doctor).contains("fail  discovery: could not use runtime directory"),
        "{}",
        stdout(&doctor)
    );

    let todo = Todo::spawn(TOKEN);
    let doctor = cli_with_root(
        &unsafe_root,
        &["--addr", todo.addr.as_str(), "--token", TOKEN, "doctor"],
    );
    assert!(!doctor.status.success());
    let doctor_stdout = stdout(&doctor);
    assert!(doctor_stdout.contains("fail  discovery:"));
    assert!(doctor_stdout.contains("ok    connect"));
    assert!(doctor_stdout.contains("ok    hello:"));
    assert!(
        doctor_stdout
            .lines()
            .any(|line| line.starts_with("ok    snapshot:") || line.starts_with("warn  snapshot:")),
        "{doctor_stdout}"
    );
    let failures = doctor_stdout
        .lines()
        .filter(|line| line.starts_with("fail"))
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1, "{doctor_stdout}");
    assert!(
        failures[0].starts_with("fail  discovery:"),
        "{doctor_stdout}"
    );

    let connect = cli_with_root(&unsafe_root, &["--connect", "fake-app", "hello"]);
    assert!(!connect.status.success());
    assert!(
        stderr(&connect).contains("unsafe runtime directory"),
        "{}",
        stderr(&connect)
    );
    assert!(!stderr(&connect).contains("no AgentHost"));

    std::fs::remove_file(&unsafe_root).expect("test fixture");
}

#[cfg(unix)]
#[test]
fn snapshot_refs_tightens_legacy_runtime_root_before_attach() {
    use std::os::unix::fs::PermissionsExt;

    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o755))
        .expect("test fixture");

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
            .expect("test fixture")
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
