//! M2: a failed CLI request still exits 1 with `error: …` on stderr, and
//! also prints a JSON envelope on stdout carrying the stable `error_code`,
//! its `hint` and, for an action, the `delivery_status`. A target action
//! reads its target back unless `--no-read-back` is given.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::thread::{self, JoinHandle};

use gpui_agent::DeliveryStatus;
use gpui_agent::protocol::{
    HelloAuth, HelloInfo, Op, PROTOCOL_VERSION, PlatformKind, Request, Response,
};
use gpui_agent::tree::{UiNode, UiTree, role};
use serde_json::Value;
use std::collections::BTreeMap;

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

fn cli_without_token(addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(["--addr", addr])
        .args(args)
        .env_remove("GPUI_AGENT_TOKEN")
        .output()
        .expect("spawn gpui-agent")
}

fn coded_error_host() -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test fixture");
    let addr = listener.local_addr().expect("test fixture").to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test fixture");
        let mut reader = BufReader::new(stream.try_clone().expect("test fixture"));
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("test fixture") == 0 {
                break;
            }
            let request: Request = serde_json::from_str(&line).expect("test fixture");
            let is_hello = matches!(request.op, Op::Hello);
            let response = if is_hello {
                let mut response = Response::ok(&request.id);
                response.hello = Some(HelloInfo {
                    protocol: PROTOCOL_VERSION,
                    app: "coded-error-test".into(),
                    platform: PlatformKind::Headless,
                    os: gpui_agent::protocol::host_os(),
                    ready: true,
                    deliveries: Vec::new(),
                    auth: HelloAuth::None,
                    bundle_id: None,
                    executable: None,
                    features: vec!["screenshot_inline".into()],
                    framework: None,
                    device: None,
                });
                response
            } else {
                let mut response = Response::err(&request.id, "opaque host failure");
                response.error_code = Some("host_specific_code".into());
                response.hint = Some("host supplied recovery hint".into());
                response
            };
            let response = serde_json::to_string(&response).expect("test fixture");
            writeln!(stream, "{response}").expect("test fixture");
            stream.flush().expect("test fixture");
            if !is_hello {
                break;
            }
        }
    });
    (addr, server)
}

fn legacy_fingerprint(node: &UiNode, parent_id: Option<&str>) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    fn feed(mut hash: u64, bytes: &[u8]) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    let mut hash = OFFSET;
    for field in [&node.id, &node.role, &node.name] {
        hash = feed(hash, field.as_bytes());
        hash = feed(hash, &[0xff]);
    }
    hash = feed(hash, &[u8::from(node.enabled), u8::from(node.visible)]);
    hash = feed(
        hash,
        &[match node.checked {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        }],
    );
    let value_hash = node
        .value
        .as_deref()
        .map_or(0u64, |value| feed(OFFSET, value.as_bytes()));
    hash = feed(hash, &value_hash.to_le_bytes());
    for state in &node.states {
        hash = feed(hash, state.as_bytes());
        hash = feed(hash, &[0xff]);
    }
    if let Some(parent) = parent_id {
        hash = feed(hash, parent.as_bytes());
    }
    feed(hash, &[u8::from(parent_id.is_some())])
}

fn legacy_secret_host() -> (String, u64, JoinHandle<bool>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test fixture");
    let addr = listener.local_addr().expect("test fixture").to_string();
    let mut raw_secret =
        UiNode::new("password-field", role::TEXTBOX, "Password").with_value("hunter2");
    raw_secret.states.push("secret".into());
    let raw_fingerprint = legacy_fingerprint(&raw_secret, None);
    let mut masked_secret = raw_secret.clone();
    masked_secret.redact_secrets();
    let tree = UiTree {
        app: "legacy-secret-host".into(),
        platform: PlatformKind::Headless,
        ready: true,
        nodes: vec![masked_secret],
        rev: None,
    };
    let server_tree = tree;
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test fixture");
        let mut reader = BufReader::new(stream.try_clone().expect("test fixture"));
        let mut type_dispatched = false;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).expect("test fixture") == 0 {
                break;
            }
            let request: Request = serde_json::from_str(&line).expect("test fixture");
            let id = request.id;
            let response = match request.op {
                Op::Hello => {
                    let mut response = Response::ok(id);
                    response.hello = Some(HelloInfo {
                        protocol: PROTOCOL_VERSION,
                        app: "legacy-secret-host".into(),
                        platform: PlatformKind::Headless,
                        os: gpui_agent::protocol::host_os(),
                        ready: true,
                        deliveries: Vec::new(),
                        auth: HelloAuth::None,
                        bundle_id: None,
                        executable: None,
                        features: Vec::new(),
                        framework: None,
                        device: None,
                    });
                    response
                }
                Op::Snapshot { .. } => {
                    let mut response = Response::ok(id);
                    response.tree = Some(server_tree.clone());
                    response.fingerprints =
                        Some(BTreeMap::from([("password-field".into(), raw_fingerprint)]));
                    response
                }
                Op::Type {
                    target,
                    expect_fingerprint,
                    ..
                } => {
                    if target != "password-field" {
                        Response::err(id, "node `password-field` not found")
                    } else if expect_fingerprint
                        .is_some_and(|fingerprint| fingerprint != raw_fingerprint)
                    {
                        let mut response = Response::err(
                            id,
                            "stale_target: `password-field` changed before dispatch",
                        );
                        response.error_code = Some("stale_target".into());
                        response.hint = Some("take a new snapshot and retry".into());
                        response.delivery_status = Some(DeliveryStatus::NotDelivered);
                        response
                    } else {
                        type_dispatched = true;
                        let mut response = Response::ok(id);
                        response.delivery_status = Some(DeliveryStatus::DeliveredUnverified);
                        response
                    }
                }
                _ => Response::err(id, "unexpected operation"),
            };
            let response = serde_json::to_string(&response).expect("test fixture");
            writeln!(stream, "{response}").expect("test fixture");
            stream.flush().expect("test fixture");
        }
        type_dispatched
    });
    (addr, raw_fingerprint, server)
}

fn failed_envelope(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("error: "), "{stderr}");
    let envelope: Value = serde_json::from_slice(&out.stdout).expect("envelope on stdout");
    assert_eq!(envelope["ok"], false, "{envelope}");
    assert!(
        stderr.contains(envelope["error"].as_str().expect("test fixture")),
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
fn click_on_a_missing_target_reports_not_found_and_not_delivered() {
    let todo = Todo::spawn(TOKEN);
    // Read-back finds no such node and does not send the click; without
    // it the host refuses the id before dispatch.
    for extra in [&[][..], &["--no-read-back"][..]] {
        let mut args = vec!["click", "no-such-node"];
        args.extend_from_slice(extra);
        let envelope = failed_envelope(&cli(&todo.addr, &args));
        assert_eq!(envelope["error_code"], "not_found", "{envelope}");
        assert_eq!(envelope["delivery_status"], "not_delivered", "{envelope}");
        assert!(envelope["hint"].is_string(), "{envelope}");
    }
}

#[test]
fn wait_until_timeout_reports_timeout() {
    let todo = Todo::spawn(TOKEN);
    let envelope = failed_envelope(&cli(
        &todo.addr,
        &["wait-until", "--id", "nope", "--timeout-ms", "100"],
    ));
    assert_eq!(envelope["error_code"], "timeout", "{envelope}");
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
fn legacy_host_rejects_stale_secret_fingerprint_before_dispatch() {
    let (addr, raw_fingerprint, server) = legacy_secret_host();
    let stale_fingerprint = (raw_fingerprint ^ 1).to_string();
    let envelope = failed_envelope(&cli_without_token(
        &addr,
        &[
            "--no-read-back",
            "type",
            "password-field",
            "replacement",
            "--expect-fingerprint",
            &stale_fingerprint,
        ],
    ));
    assert_eq!(envelope["error_code"], "stale_target", "{envelope}");
    assert_eq!(envelope["delivery_status"], "not_delivered", "{envelope}");
    assert!(!server.join().expect("test fixture"));
}

#[test]
fn legacy_snapshot_refs_do_not_print_or_save_secret_fingerprints() {
    let (addr, raw_fingerprint, server) = legacy_secret_host();
    let snapshot = cli_without_token(&addr, &["snapshot", "--refs", "--fingerprints"]);
    assert_eq!(snapshot.status.code(), Some(0), "{snapshot:?}");
    let stdout = String::from_utf8_lossy(&snapshot.stdout);
    assert!(
        !stdout.contains(&format!("fp={raw_fingerprint}")),
        "{stdout}"
    );
    assert!(!server.join().expect("test fixture"));

    let guarded = cli_without_token(
        &addr,
        &["--guard", "--no-read-back", "type", "@e1", "replacement"],
    );
    assert_eq!(guarded.status.code(), Some(1), "{guarded:?}");
    let stderr = String::from_utf8_lossy(&guarded.stderr);
    assert!(stderr.contains("no fingerprint saved"), "{stderr}");
    assert!(!stderr.contains("connect"), "{stderr}");
}

#[test]
fn unreachable_host_reports_connect_failed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("test fixture");
    let addr = listener.local_addr().expect("test fixture").to_string();
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
fn string_api_commands_report_connect_failed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("test fixture");
    let addr = listener.local_addr().expect("test fixture").to_string();
    drop(listener);
    for command in ["events", "snapshot", "keybindings"] {
        let envelope = failed_envelope(&cli(&addr, &["--reply-timeout-ms", "300", command]));
        assert_eq!(
            envelope["error_code"], "connect_failed",
            "{command}: {envelope}"
        );
    }
}

#[test]
fn action_reads_back_by_default() {
    let todo = Todo::spawn(TOKEN);
    let out = cli(&todo.addr, &["type", "todo-input", "Milk"]);
    assert!(out.status.success(), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).expect("test fixture");
    assert_eq!(reply["delivery_status"], "delivered_verified", "{reply}");
    assert_eq!(reply["after"]["value"], "Milk", "{reply}");

    let out = cli(&todo.addr, &["type", "todo-input", "!", "--no-read-back"]);
    assert!(out.status.success(), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).expect("test fixture");
    assert_eq!(reply["delivery_status"], "delivered_unverified", "{reply}");
    assert!(reply.get("after").is_none(), "{reply}");
}

#[test]
fn keybinding_reports_delivery_status() {
    let todo = Todo::spawn(TOKEN);
    let out = cli(
        &todo.addr,
        &[
            "keybinding",
            "--id",
            todo_core::ids::KEY_GO_SETTINGS,
            "--scope",
            "global",
        ],
    );
    assert!(out.status.success(), "{out:?}");
    let reply: Value = serde_json::from_slice(&out.stdout).expect("test fixture");
    assert_eq!(reply["delivery_status"], "delivered_unverified", "{reply}");
}

#[test]
fn cli_preserves_host_error_metadata_for_legacy_operations() {
    let screenshot = std::env::temp_dir()
        .join("gpui-agent-host-error.png")
        .to_string_lossy()
        .into_owned();
    let cases = [
        vec!["snapshot".to_string()],
        vec!["screenshot".into(), "--out".into(), screenshot.clone()],
        vec![
            "screenshot".into(),
            "--out".into(),
            screenshot,
            "--inline".into(),
        ],
        vec!["wait".into()],
        vec![
            "wait-until".into(),
            "--id".into(),
            "missing".into(),
            "--timeout-ms".into(),
            "50".into(),
        ],
        vec![
            "keybinding".into(),
            "--id".into(),
            "custom.binding".into(),
            "--scope".into(),
            "global".into(),
        ],
    ];

    for args in cases {
        let (addr, server) = coded_error_host();
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        let envelope = failed_envelope(&cli_without_token(&addr, &args));
        assert_eq!(envelope["error_code"], "host_specific_code", "{envelope}");
        assert_eq!(
            envelope["hint"], "host supplied recovery hint",
            "{envelope}"
        );
        if args[0] == "keybinding" {
            assert!(envelope["delivery_status"].is_string(), "{envelope}");
        }
        server.join().expect("test fixture");
    }
}
