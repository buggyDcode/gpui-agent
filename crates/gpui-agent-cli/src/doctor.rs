//! `targets` and `doctor`: what can be driven from here, and why a host
//! that should answer does not.

use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use gpui_agent::client::{AgentClient, NO_REPLY};
use gpui_agent::protocol::Op;
use gpui_agent::tree::UiNode;
use serde_json::{Value, json};

/// Hosts that published themselves on this machine, and the devices `adb`
/// sees with the ports forwarded to them.
pub fn targets() -> (Value, Option<gpui_agent::ResolveError>) {
    let (hosts, discovery_error) = match gpui_agent::live_hosts(&gpui_agent::runtime_root()) {
        Ok(hosts) => (hosts, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    (
        json!({ "hosts": hosts, "adb": adb_devices(), "simulators": simulator_devices() }),
        discovery_error,
    )
}

pub fn check_discovery(root: &Path) -> bool {
    match gpui_agent::live_hosts(root) {
        Ok(_) => true,
        Err(error) => {
            println!("fail  discovery: {error}");
            false
        }
    }
}

fn adb(args: &[&str]) -> Option<String> {
    let out = Command::new("adb").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `None` when `adb` is not installed or does not answer.
fn adb_devices() -> Option<Vec<Value>> {
    let devices = adb(&["devices"])?;
    let forwards = adb(&["forward", "--list"]).unwrap_or_default();
    Some(parse_adb(&devices, &forwards))
}

fn parse_adb(devices: &str, forwards: &str) -> Vec<Value> {
    devices
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next()?;
            // `<serial> tcp:<local> tcp:<remote>`
            let forwarded: Vec<String> = forwards
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split_whitespace();
                    (parts.next()? == serial).then_some(())?;
                    let local = parts.next()?.strip_prefix("tcp:")?;
                    Some(format!("127.0.0.1:{local}"))
                })
                .collect();
            Some(json!({ "serial": serial, "state": state, "forwarded": forwarded }))
        })
        .collect()
}

/// `None` when `xcrun` is not installed or does not answer.
///
/// Booted iOS Simulators and the hosts each publishes. A host inside a
/// Simulator writes its discovery record under the device's own
/// `data/tmp`, outside this machine's `runtime_root`, so `hosts` never
/// lists it.
fn simulator_devices() -> Option<Vec<Value>> {
    let out = Command::new("xcrun")
        .args(["simctl", "list", "devices", "booted", "-j"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let listing: Value = serde_json::from_slice(&out.stdout).ok()?;
    let mut sims = Vec::new();
    for devices in listing.get("devices")?.as_object()?.values() {
        for device in devices.as_array().into_iter().flatten() {
            sims.push(json!({
                "udid": device.get("udid").and_then(Value::as_str).unwrap_or_default(),
                "name": device.get("name").and_then(Value::as_str).unwrap_or_default(),
                "state": device.get("state").and_then(Value::as_str).unwrap_or_default(),
                "hosts": device
                    .get("dataPath")
                    .and_then(Value::as_str)
                    .map(simulator_device_hosts)
                    .unwrap_or_default(),
            }));
        }
    }
    Some(sims)
}

/// Host records a Simulator app publishes under its container tmp dir
/// (`<dataPath>/tmp/gpui-agent-<uid>/hosts`).
fn simulator_device_hosts(data_path: &str) -> Vec<Value> {
    let mut hosts = Vec::new();
    let Ok(roots) = std::fs::read_dir(Path::new(data_path).join("tmp")) else {
        return hosts;
    };
    for entry in roots.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("gpui-agent-")
        {
            continue;
        }
        let Ok(records) = gpui_agent::live_hosts(&entry.path()) else {
            continue;
        };
        hosts.extend(
            records
                .into_iter()
                .filter_map(|rec| serde_json::to_value(rec).ok()),
        );
    }
    hosts
}

fn count(nodes: &[UiNode]) -> usize {
    nodes.iter().map(|node| 1 + count(&node.children)).sum()
}

/// Runs the checks in order and prints one line each; `false` when the host
/// cannot be driven as it stands.
pub fn doctor(addr: SocketAddr, mut client: AgentClient, has_token: bool, root: &Path) -> bool {
    let discovery_ok = check_discovery(root);
    let connect = Duration::from_secs(2);
    if let Err(err) = TcpStream::connect_timeout(&addr, connect) {
        println!("fail  connect {addr}: {err}");
        println!("      Start the app with GPUI_AGENT=1 (a development build).");
        println!("      Android: `adb forward tcp:{0} tcp:{0}`.", addr.port());
        println!("      `gpui-agent targets` lists hosts and devices found here.");
        return false;
    }
    println!("ok    connect {addr}");

    let hello = match client.expect_ok(Op::Hello) {
        Ok(resp) => resp.hello,
        Err(err) => {
            println!("fail  hello: {err}");
            if err.starts_with(NO_REPLY) {
                println!("      The port is open but nothing answered: a forwarded port");
                println!("      with no host behind it, or an app too busy to reply.");
            } else if has_token {
                println!("      If this is an auth error, the token differs from the host's.");
            } else {
                println!("      No token given: set GPUI_AGENT_TOKEN to the host's value.");
            }
            return false;
        }
    };
    let Some(hello) = hello else {
        println!("fail  hello: the reply carried no hello");
        return false;
    };
    println!(
        "ok    hello: {app} ({framework}, {platform:?} on {os}), protocol {protocol}",
        app = hello.app,
        framework = hello.framework.as_deref().unwrap_or("framework not stated"),
        platform = hello.platform,
        os = hello.os,
        protocol = hello.protocol,
    );
    if !hello.ready {
        println!("warn  the host says it is not ready yet (`gpui-agent wait`)");
    }
    if hello.features.iter().any(|f| f == "screenshot_inline") {
        println!("ok    features: {}", hello.features.join(", "));
    } else {
        println!("warn  a 0.1 host: press, scroll, hover, drag and viewport go through `invoke`;");
        println!(
            "      swipe, long-press, pinch, paste, system and inline screenshots are refused"
        );
    }

    let start = Instant::now();
    match client.snapshot() {
        Ok(resp) => {
            let took = start.elapsed();
            let nodes = resp.tree.map_or(0, |tree| count(&tree.nodes));
            let slow = took > Duration::from_secs(2);
            println!(
                "{}  snapshot: {nodes} nodes in {} ms",
                if slow { "warn" } else { "ok  " },
                took.as_millis()
            );
            if slow {
                println!("      Slow answers: pass --reply-timeout-ms on this host.");
            }
            discovery_ok
        }
        Err(err) => {
            println!("fail  snapshot: {err}");
            if err.starts_with(NO_REPLY) {
                println!("      The app answers hello but not its UI thread in time:");
                println!("      retry with a larger --reply-timeout-ms, or pause what animates.");
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_devices_carry_their_forwarded_ports() {
        let devices = "List of devices attached\nemulator-5554\tdevice\nR58M\tunauthorized\n\n";
        let forwards = "emulator-5554 tcp:17421 tcp:17421\nother tcp:1 tcp:1\n";
        assert_eq!(
            parse_adb(devices, forwards),
            vec![
                json!({"serial": "emulator-5554", "state": "device", "forwarded": ["127.0.0.1:17421"]}),
                json!({"serial": "R58M", "state": "unauthorized", "forwarded": []}),
            ]
        );
    }
}
