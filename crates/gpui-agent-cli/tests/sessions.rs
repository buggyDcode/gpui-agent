//! Named CLI sessions persist a host address and token, not a process.

mod common;

use std::net::TcpListener;
use std::process::{Command, Output};

use common::{RuntimeRoot, Todo};
use serde_json::Value;

const TOKEN: &str = "named-session-secret";

fn cli(runtime: &RuntimeRoot, args: &[&str]) -> Output {
    cli_with_env_token(runtime, args, None)
}

fn cli_with_env_token(runtime: &RuntimeRoot, args: &[&str], token: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gpui-agent"));
    command
        .args(args)
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .env_remove("GPUI_AGENT_SESSION")
        .env_remove("GPUI_AGENT_ADDR")
        .env_remove("GPUI_AGENT_CONNECT")
        .env_remove("GPUI_AGENT_TOKEN")
        .env_remove("GPUI_AGENT_ALLOW_REMOTE");
    if let Some(token) = token {
        command.env("GPUI_AGENT_TOKEN", token);
    }
    command.output().expect("spawn gpui-agent")
}

fn cli_with_env_addr(runtime: &RuntimeRoot, args: &[&str], addr: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(args)
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .env("GPUI_AGENT_ADDR", addr)
        .env_remove("GPUI_AGENT_SESSION")
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
fn runtime_roots_created_concurrently_are_distinct() {
    let threads: Vec<_> = (0..16)
        .map(|_| std::thread::spawn(RuntimeRoot::new))
        .collect();
    let roots: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().expect("create isolated runtime root"))
        .collect();
    let paths: std::collections::HashSet<_> =
        roots.iter().map(|root| root.path().to_path_buf()).collect();
    assert_eq!(paths.len(), roots.len());
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
    assert_eq!(
        sessions.as_array().expect("test fixture").len(),
        1,
        "{sessions}"
    );
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
    let dead_addr = listener.local_addr().expect("test fixture").to_string();
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

#[test]
fn saved_token_precedes_environment_but_command_line_token_overrides_it() {
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

    let hello = cli_with_env_token(&runtime, &["--session", "s", "hello"], Some("wrong"));
    assert!(
        hello.status.success(),
        "saved token should take precedence over GPUI_AGENT_TOKEN: {}",
        stderr(&hello)
    );

    let explicit_wrong = cli_with_env_token(
        &runtime,
        &["--session", "s", "--token", "wrong", "hello"],
        Some(TOKEN),
    );
    assert!(
        !explicit_wrong.status.success(),
        "a command-line token should override the saved token"
    );
}

#[test]
fn named_session_conflicts_with_command_line_selectors_but_ignores_env_addr() {
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

    let addr_conflict = cli(
        &runtime,
        &["--session", "s", "hello", "--addr", "127.0.0.1:9"],
    );
    assert!(!addr_conflict.status.success());
    assert!(
        stderr(&addr_conflict).contains("command-line --addr"),
        "{}",
        stderr(&addr_conflict)
    );

    let connect_conflict = cli(
        &runtime,
        &["--connect", "some-app", "hello", "--session", "s"],
    );
    assert!(!connect_conflict.status.success());
    assert!(
        stderr(&connect_conflict).contains("command-line --connect"),
        "{}",
        stderr(&connect_conflict)
    );

    let env_addr = cli_with_env_addr(&runtime, &["--session", "s", "hello"], "127.0.0.1:9");
    assert!(
        env_addr.status.success(),
        "the saved session should override GPUI_AGENT_ADDR: {}",
        stderr(&env_addr)
    );
    assert!(serde_json::from_slice::<Value>(&env_addr.stdout).is_ok());

    let detach_conflict = cli(
        &runtime,
        &["--session", "s", "--addr", "127.0.0.1:9", "detach"],
    );
    assert!(!detach_conflict.status.success());
    assert!(
        stderr(&detach_conflict).contains("command-line --addr"),
        "{}",
        stderr(&detach_conflict)
    );
    let listed = cli(&runtime, &["sessions"]);
    let sessions: Value = serde_json::from_slice(&listed.stdout).expect("sessions JSON");
    assert_eq!(
        sessions.as_array().expect("test fixture").len(),
        1,
        "{sessions}"
    );
    assert_eq!(sessions[0]["name"], "s");
}

#[test]
fn offline_recipe_validation_ignores_a_missing_session() {
    let runtime = RuntimeRoot::new();
    let recipe = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/recipes/todo-crud.json");
    let schema = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/schemas/todo.json");
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(["recipe", "validate"])
        .arg(recipe)
        .arg("--schema")
        .arg(schema)
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .env("GPUI_AGENT_SESSION", "missing")
        .env_remove("GPUI_AGENT_ADDR")
        .env_remove("GPUI_AGENT_CONNECT")
        .env_remove("GPUI_AGENT_TOKEN")
        .env_remove("GPUI_AGENT_ALLOW_REMOTE")
        .output()
        .expect("spawn gpui-agent");
    assert!(
        output.status.success(),
        "offline recipe validation should not resolve a host: {}",
        stderr(&output)
    );
    assert!(stdout(&output).contains("\"ok\": true"));
}
