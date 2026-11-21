//! M4: `type --stdin` / `set-value --stdin` against a real todo host —
//! the text comes from stdin, not the command line.

use std::io::Write;
use std::process::{Command, Output, Stdio};

mod common;
use common::{RuntimeRoot, Todo};

const TOKEN: &str = "m4-stdin-token";

fn cli_with_stdin(runtime: &RuntimeRoot, addr: &str, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gpui-agent"))
        .args(["--addr", addr, "--token", TOKEN])
        .args(args)
        .env_remove("GPUI_AGENT_TOKEN")
        .env("GPUI_AGENT_RUNTIME_DIR", runtime.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn gpui-agent");
    child
        .stdin
        .take()
        .expect("test fixture")
        .write_all(stdin.as_bytes())
        .expect("test fixture");
    child.wait_with_output().expect("gpui-agent output")
}

#[test]
fn type_and_set_value_read_the_text_from_stdin() {
    let todo = Todo::spawn(TOKEN);
    let runtime = RuntimeRoot::new();

    // `echo`'s trailing newline is not typed.
    let typed = cli_with_stdin(
        &runtime,
        &todo.addr,
        &["type", "--stdin", "todo-input"],
        "milk\n",
    );
    assert!(typed.status.success(), "{typed:?}");
    assert_eq!(todo.draft(), "milk");

    let set = cli_with_stdin(
        &runtime,
        &todo.addr,
        &["set-value", "todo-input", "--stdin"],
        "eggs",
    );
    assert!(set.status.success(), "{set:?}");
    assert_eq!(todo.draft(), "eggs");
}

#[test]
fn stdin_and_a_text_argument_are_exclusive() {
    let runtime = RuntimeRoot::new();
    // Nothing listens here: clap must refuse before any connection.
    let both = cli_with_stdin(
        &runtime,
        "127.0.0.1:9",
        &["type", "--stdin", "todo-input", "milk"],
        "",
    );
    assert_eq!(both.status.code(), Some(2), "{both:?}");
    let neither = cli_with_stdin(&runtime, "127.0.0.1:9", &["paste", "todo-input"], "");
    assert_eq!(neither.status.code(), Some(2), "{neither:?}");
}
