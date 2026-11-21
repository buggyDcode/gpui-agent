# gpui-agent

An agent control plane for native apps: the app embeds a small `AgentHost`,
publishes a **semantic UI tree** with stable ids, and accepts **scripted
actions** over loopback JSON. AI agents, CLIs and MCP clients drive the app
without a human mouse or keyboard.

GPUI Kit and Native Shell apps are GPU-native surfaces — there is no DOM for
Playwright or Chrome DevTools Protocol to attach to. gpui-agent is the
in-process alternative: the app publishes its own tree (ids, roles, names,
bounds, state) and answers protocol ops. The wire is **protocol v2**, NDJSON
on `127.0.0.1`; capability gating is via `hello.features`, so old hosts and
new clients degrade gracefully instead of breaking.

The protocol is **framework-neutral**. Any app that implements an `AgentHost`
can be driven — this is how [nsh](https://github.com/hexuria/nsh) apps are
driven on every platform nsh targets.

## Platform support

`gpui-agent targets` lists live hosts published on the machine (plus adb
devices and forwarded ports); `gpui-agent doctor` says why a host that should
answer does not.

| Host | Platform | Status |
| --- | --- | --- |
| nsh (AppKit) | macOS | Conformance suite passes |
| nsh (Win32) | Windows | Conformance suite passes |
| nsh (GTK 4) | Linux | Conformance suite passes |
| nsh | Web (browser) | Conformance suite passes |
| nsh | Android (emulator/device, via `adb forward`) | Conformance suite passes |
| nsh (UIKit) | iOS Simulator | Host discovery via `targets` |
| todo-headless | Any (demo daemon) | Conformance suite passes |
| todo (GPUI Kit) | Desktop demo | Reference `AgentHost` |

`python3 scripts/conformance.py --addr 127.0.0.1:17421` runs the required
protocol checks against any host.

## Install

```bash
cargo install gpui-agent-cli   # installs the `gpui-agent` binary
```

`cargo binstall gpui-agent-cli` fetches the prebuilt release binary. The
crate is Apache-2.0 and needs no GPUI/Metal/Vulkan to build.

## Quick start

```bash
# terminal 1 — a host (demo daemon shipped here)
export GPUI_AGENT=1 GPUI_AGENT_TOKEN=dev-secret
cargo run -p todo-headless -- serve

# terminal 2 — same token
export GPUI_AGENT_TOKEN=dev-secret
gpui-agent wait
gpui-agent hello
gpui-agent snapshot --refs          # @e1 button "Add" …; every op takes @eN refs
gpui-agent set-value todo-input "Buy milk"
gpui-agent click todo-add
gpui-agent assert --id todo-item-1 --name "Buy milk" --checked false
gpui-agent click nav-settings
gpui-agent assert --id page-settings --role page
gpui-agent shutdown
```

Ops: `wait`, `wait-until`, `hello`, `snapshot`, `screenshot`, `click`,
`press`, `type`, `set-value`, `key`, `keybinding`, `keybindings`, `assert`,
`invoke`, `events`, `shutdown`, plus `targets`, `doctor`, `sessions`,
`recipe` and `mcp`. App-specific verbs belong in the app (stable ids +
`invoke` names) or in agent prompts, never as CLI subcommands.

For repeated work, attach once (`gpui-agent --session todo attach
127.0.0.1:17421`) and reuse `--session todo` — a named session stores the
address and token on disk, private to the current user.

## Recipes and MCP

One `recipe run` invocation batches many ops on one kept TCP session —
~600× cheaper than a process spawn per op ([docs/PERF.md](docs/PERF.md)).
`gpui-agent mcp` exposes the same ops as MCP stdio tools:

```json
{
  "mcpServers": {
    "gpui-agent": {
      "command": "gpui-agent",
      "args": ["mcp"],
      "env": { "GPUI_AGENT_ADDR": "127.0.0.1:17421", "GPUI_AGENT_TOKEN": "dev-secret" }
    }
  }
}
```

`recipe run` and `mcp` require a non-empty `GPUI_AGENT_TOKEN` or `--token`,
set to the same value on the host. `GPUI_AGENT_INSECURE_NO_TOKEN=1` is the
only untokened loopback (demo use). Trust model: [docs/SECURITY.md](docs/SECURITY.md).

## How it works

1. **Perceive.** `snapshot` returns the semantic tree the app published —
   stable ids, roles, names, state. Do not scrape pixels to decide.
2. **Act.** `click` / `type` / `set-value` / `key` / `keybinding` / `invoke`.
   Semantic delivery is the default; `--delivery virtual` synthesizes real
   input events in the app window (hover, hit-test, focus, IME).
3. **Verify.** A mutation reply's `after` holds the target's new state
   (`delivered_verified`); `assert` checks the tree. `screenshot` returns a
   real PNG where the platform supports it, `screenshot_unavailable` where
   it does not — never a fake image.

The TCP thread never touches app objects: the desktop bridge is a mailbox
the UI thread drains. Wire format, ops and feature flags:
[docs/PROTOCOL.md](docs/PROTOCOL.md). Embedding `AgentHost` in your own app:
[docs/INTEGRATING.md](docs/INTEGRATING.md) and [docs/SDK.md](docs/SDK.md).

## Layout

```
apps/todo, apps/todo-headless   GPUI Kit demo: GUI client + logic daemon
crates/gpui-agent               Embeddable SDK: protocol, server, client, TestHost
crates/gpui-agent-cli           gpui-agent CLI + MCP stdio shim
crates/gpui-agent-recipe        Recipe batching + prose resolver
crates/gpui-agent-policy        Local agent policy (run --goal; default `instinct`)
crates/gpui-agent-bench         Bench harness (local + resolve arms)
crates/todo-core                Demo store and semantic ids
scripts/smoke.sh                Full CRUD against the headless host
scripts/conformance.py          Protocol conformance suite for any host
docs/                           PROTOCOL, INTEGRATING, SDK, RECIPES,
                                RECORDING, SECURITY, PERF
```

## License

Apache-2.0. See [LICENSE](LICENSE).
