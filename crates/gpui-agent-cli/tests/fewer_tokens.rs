//! M3 against a real todo host: fingerprint guards, `--wait-for` and
//! outline snapshots.

use std::process::{Command, Output};

use serde_json::Value;

mod common;
use common::{RuntimeRoot, Todo};

const TOKEN: &str = "m3-fewer-tokens-token";

fn cli(runtime: &RuntimeRoot, addr: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(["--addr", addr, "--token", TOKEN])
        .args(args)
        .env_remove("GPUI_AGENT_TOKEN")
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .output()
        .expect("spawn gpui-agent")
}

fn ok_json(out: &Output) -> Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("JSON on stdout")
}

fn failed_json(out: &Output) -> Value {
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("envelope on stdout")
}

fn text(out: &Output) -> String {
    assert!(out.status.success(), "{out:?}");
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// The ref the `--refs` outline gave the line containing `needle`.
fn ref_of(outline: &str, needle: &str) -> String {
    let line = outline
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no line with {needle} in {outline}"));
    line.split_whitespace().next().unwrap().to_string()
}

#[test]
fn an_expected_fingerprint_refuses_a_changed_target() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    let snap = ok_json(&cli(&runtime, &todo.addr, &["snapshot", "--fingerprints"]));
    let fp = snap["fingerprints"]["todo-input"].as_u64().unwrap();

    let stale = failed_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "set-value",
            "todo-input",
            "x",
            "--expect-fingerprint",
            &(fp ^ 1).to_string(),
        ],
    ));
    assert_eq!(stale["error_code"], "stale_target", "{stale}");
    assert_eq!(stale["delivery_status"], "not_delivered");
    assert_eq!(todo.draft(), "");

    let done = ok_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "type",
            "todo-input",
            "milk",
            "--expect-fingerprint",
            &fp.to_string(),
        ],
    ));
    assert_eq!(done["delivery_status"], "delivered_verified", "{done}");
    assert_eq!(todo.draft(), "milk");
}

#[test]
fn guard_sends_the_saved_fingerprint_of_a_ref() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    let outline = text(&cli(&runtime, &todo.addr, &["snapshot", "--refs"]));
    assert!(!outline.contains("fp="), "{outline}");
    let input = ref_of(&outline, "textbox");

    // Without --guard a ref is just an id.
    ok_json(&cli(&runtime, &todo.addr, &["type", &input, "milk"]));
    // The value changed, so the saved fingerprint is stale now.
    let stale = failed_json(&cli(
        &runtime,
        &todo.addr,
        &["--guard", "key", &input, "Enter"],
    ));
    assert_eq!(stale["error_code"], "stale_target", "{stale}");
    assert!(todo.titles().is_empty());

    let outline = text(&cli(
        &runtime,
        &todo.addr,
        &["snapshot", "--refs", "--fingerprints"],
    ));
    let input = ref_of(&outline, "textbox");
    assert!(outline.contains(" fp="), "{outline}");
    ok_json(&cli(
        &runtime,
        &todo.addr,
        &["--guard", "key", &input, "Enter"],
    ));
    assert_eq!(todo.titles(), ["milk"]);
}

#[test]
fn guard_does_not_use_a_wait_ref_for_a_plain_action_target() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    let outline = text(&cli(
        &runtime,
        &todo.addr,
        &["snapshot", "--refs", "--fingerprints"],
    ));
    let input = ref_of(&outline, "textbox");

    ok_json(&cli(
        &runtime,
        &todo.addr,
        &["type", "todo-input", "before"],
    ));
    let result = ok_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "--guard",
            "--wait-for",
            &input,
            "click",
            "todo-input",
        ],
    ));
    assert_eq!(result["delivery_status"], "delivered_verified", "{result}");
}

#[test]
fn wait_for_returns_the_new_tree_in_compact_form() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();
    let added = ok_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "invoke",
            "todo.add",
            "--arg",
            "title=Milk",
            "--wait-for",
            "todo-item-1",
        ],
    ));
    let compact = added["compact"].as_str().unwrap();
    assert!(
        compact.contains("todo-item-1 listitem \"Milk\""),
        "{compact}"
    );

    let gone = ok_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "invoke",
            "todo.delete",
            "--arg",
            "id=1",
            "--wait-for-gone",
            "todo-item-1",
        ],
    ));
    assert!(!gone["compact"].as_str().unwrap().contains("todo-item-1"));

    let timed_out = failed_json(&cli(
        &runtime,
        &todo.addr,
        &[
            "set-value",
            "todo-input",
            "x",
            "--wait-for",
            "todo-item-9",
            "--wait-timeout-ms",
            "200",
        ],
    ));
    assert_eq!(timed_out["error_code"], "timeout", "{timed_out}");
    // The action itself went through.
    assert_eq!(timed_out["delivery_status"], "delivered_verified");
    assert_eq!(todo.draft(), "x");
}

#[test]
fn outline_snapshots_cut_depth_and_root() {
    let todo = Todo::spawn(TOKEN);
    todo.seed(&["Milk", "Eggs"]);
    let runtime = RuntimeRoot::new();
    let full = text(&cli(&runtime, &todo.addr, &["snapshot"]));
    let one = text(&cli(&runtime, &todo.addr, &["snapshot", "--depth", "1"]));
    assert!(
        one.len() * 4 < full.len(),
        "{} vs {}",
        one.len(),
        full.len()
    );
    let one: Value = serde_json::from_str(&one).unwrap();
    let window = &one["tree"]["nodes"][0];
    assert!(window.get("children").is_none(), "{window}");
    assert!(window["children_count"].as_u64().unwrap() > 0);

    let item = ok_json(&cli(
        &runtime,
        &todo.addr,
        &["snapshot", "--root", "todo-item-2"],
    ));
    assert_eq!(item["tree"]["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(item["tree"]["nodes"][0]["id"], "todo-item-2");

    let missing = failed_json(&cli(&runtime, &todo.addr, &["snapshot", "--root", "nope"]));
    assert_eq!(missing["error_code"], "bad_request", "{missing}");

    // --refs: a cut node is listed with its count, and its ref can be the
    // next root.
    let outline = text(&cli(
        &runtime,
        &todo.addr,
        &["snapshot", "--refs", "--depth", "3"],
    ));
    let list = ref_of(&outline, "children_count=");
    assert!(!outline.contains("Eggs"), "{outline}");
    let below = text(&cli(
        &runtime,
        &todo.addr,
        &["snapshot", "--refs", "--root", &list],
    ));
    assert!(below.contains("Eggs"), "{below}");
}
