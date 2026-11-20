//! Named CLI sessions persist a host address and token, not a process.

mod common;

use std::net::TcpListener;
use std::process::{Command, Output};

use common::{RuntimeRoot, Todo};
use serde_json::Value;

const TOKEN: &str = "named-session-secret";

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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn named_sessions_attach_reuse_list_detach_and_do_not_save_failed_attach() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();

    let attached = cli(
        &runtime,
        &["--session", "s", "--token", TOKEN, "attach", &todo.addr],
    );
    assert!(
        attached.status.success(),
        "attach failed: {}",
        stderr(&attached)
    );
    assert!(stdout(&attached).contains(&format!("session s: {}", todo.addr)));

    let hello = cli(&runtime, &["--session", "s", "hello"]);
    assert!(
        hello.status.success(),
        "session hello failed: {}",
        stderr(&hello)
    );
    assert!(stderr(&hello).contains(&format!("gpui-agent: session s -> {}", todo.addr)));
    assert!(serde_json::from_slice::<Value>(&hello.stdout).is_ok());

    let listed = cli(&runtime, &["sessions"]);
    assert!(
        listed.status.success(),
        "sessions failed: {}",
        stderr(&listed)
    );
    let sessions: Value = serde_json::from_slice(&listed.stdout).expect("sessions JSON");
    assert_eq!(sessions.as_array().unwrap().len(), 1, "{sessions}");
    assert_eq!(sessions[0]["name"], "s");
    assert_eq!(sessions[0]["addr"], todo.addr);
    assert_eq!(sessions[0]["token"], "yes");
    assert!(
        !format!("{}{}", stdout(&listed), stderr(&listed)).contains(TOKEN),
        "sessions output disclosed the token"
    );

    let detached = cli(&runtime, &["--session", "s", "detach"]);
    assert!(
        detached.status.success(),
        "detach failed: {}",
        stderr(&detached)
    );
    assert!(stdout(&detached).contains("detached"));

    let missing = cli(&runtime, &["--session", "s", "hello"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains(
            "no session named \"s\"; create it with: gpui-agent --session <name> attach <target>"
        ),
        "{}",
        stderr(&missing)
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve dead port");
    let dead_addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    let failed_attach = cli(
        &runtime,
        &["--session", "dead", "--token", TOKEN, "attach", &dead_addr],
    );
    assert!(!failed_attach.status.success());

    let listed = cli(&runtime, &["sessions"]);
    assert!(
        listed.status.success(),
        "sessions failed: {}",
        stderr(&listed)
    );
    let sessions: Value = serde_json::from_slice(&listed.stdout).expect("sessions JSON");
    assert_eq!(sessions, serde_json::json!([]));
}
