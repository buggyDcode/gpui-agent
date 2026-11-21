//! M2: a failed CLI request still exits 1 with `error: …` on stderr, and
//! also prints a JSON envelope on stdout carrying the stable `error_code`,
//! its `hint` and, for an action, the `delivery_status`. A target action
//! reads its target back unless `--no-read-back` is given.

use std::process::{Command, Output};

use serde_json::Value;

mod common;
use common::Todo;

const TOKEN: &str = "m2-envelope-token";

fn cli(addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(["--addr", addr, "--token", TOKEN])
        .args(args)
        .env_remove("GPUI_AGENT_TOKEN")
        .output()
        .expect("spawn gpui-agent")
}

fn failed_envelope(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("error: "), "{stderr}");
    let envelope: Value = serde_json::from_slice(&out.stdout).expect("envelope on stdout");
    assert_eq!(envelope["ok"], false, "{envelope}");
    assert!(
        stderr.contains(envelope["error"].as_str().unwrap()),
        "stderr and envelope disagree: {stderr} / {envelope}"
    );
    envelope
}

#[test]
fn missing_target_reports_not_found() {
    let todo = Todo::spawn(TOKEN);
    let envelope = failed_envelope(&cli(&todo.addr, &["assert", "--id", "no-such-node"]));
    assert_eq!(envelope["error_code"], "not_found", "{envelope}");
    assert!(envelope["hint"].is_string(), "{envelope}");
    // An assert does not act, so nothing was or wasn't delivered.
    assert!(envelope.get("delivery_status").is_none(), "{envelope}");
}

#[test]
fn stale_fingerprint_reports_stale_target_and_not_delivered() {
    let todo = Todo::spawn(TOKEN);
    let envelope = failed_envelope(&cli(
        &todo.addr,
        &["click", "todo-add", "--expect-fingerprint", "1"],
    ));
    assert_eq!(envelope["error_code"], "stale_target", "{envelope}");
    assert_eq!(
        envelope["hint"], "take a new snapshot and retry",
        "{envelope}"
    );
    assert_eq!(envelope["delivery_status"], "not_delivered", "{envelope}");
}

#[test]
fn unreachable_host_reports_connect_failed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    // The client retries a refused connect until its reply timeout.
    let envelope = failed_envelope(&cli(
        &addr,
        &["--reply-timeout-ms", "300", "click", "todo-add"],
    ));
    assert_eq!(envelope["error_code"], "connect_failed", "{envelope}");
    assert_eq!(envelope["delivery_status"], "not_delivered", "{envelope}");
}

#[test]
fn action_reads_back_by_default() {
    let todo = Todo::spawn(TOKEN);
    let out = cli(&todo.addr, &["type", "todo-input", "Milk"]);
    assert!(out.status.success(), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["delivery_status"], "delivered_verified", "{reply}");
    assert_eq!(reply["after"]["value"], "Milk", "{reply}");

    let out = cli(&todo.addr, &["type", "todo-input", "!", "--no-read-back"]);
    assert!(out.status.success(), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["delivery_status"], "delivered_unverified", "{reply}");
    assert!(reply.get("after").is_none(), "{reply}");
}
