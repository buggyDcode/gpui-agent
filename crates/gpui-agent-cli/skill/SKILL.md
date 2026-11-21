---
name: gpui-agent
description: Drive a running GPUI or Native Shell (nsh) app on desktop, web or Android with the gpui-agent CLI. Use to click, type, scroll, swipe, read the UI tree or take screenshots of an app started with GPUI_AGENT=1.
---

# gpui-agent

`gpui-agent` talks to an app that started its agent server (`GPUI_AGENT=1`, a
development build). It reads the app's semantic tree and acts on nodes by id.
It does not click pixels on the screen and cannot attach to an app that did
not start the server.

## Connect

```bash
export GPUI_AGENT_TOKEN=...                # the value the host was started with
gpui-agent targets                         # hosts on this machine, adb devices, forwarded ports
gpui-agent --session app attach 127.0.0.1:17421
gpui-agent --session app doctor            # address, token, capabilities, answer time
```

- Android: `adb forward tcp:17421 tcp:17421`, then use `127.0.0.1:17421`.
- For repeated work, keep using `--session app`; `gpui-agent sessions` lists
  saved sessions without revealing tokens. A session is a stored connection
  record, not a background process. Use a different name for each app.
- The saved token takes precedence over `GPUI_AGENT_TOKEN`; pass `--token` to
  override it.
- Do not rely on `--connect` for a saved workflow, which picks whichever
  matching app is running.
- `no reply from … within 8000 ms` means the app is up but busy. Raise
  `--reply-timeout-ms` (or `GPUI_AGENT_REPLY_TIMEOUT_MS`), or pause what animates.

## The loop

1. `gpui-agent --session app snapshot --refs` prints one line per node with a ref:
   `@e6 button "Pause the market"`.
2. Act on a ref: `gpui-agent --session app press @e6`.
3. Run `gpui-agent --session app snapshot --refs` again and read the result.
   Refs are renumbered by every `snapshot --refs`; never reuse one from before
   the screen changed.

Use `gpui-agent --session app snapshot` for the full JSON tree (bounds, states, ids)
and `gpui-agent --session app assert` / `gpui-agent --session app wait-until` to
check a condition without reading the tree.

## Actions

| Command | Does |
|---|---|
| `press <node>` or `press --x X --y Y` | Tap or click |
| `type <node> <text>`, `set-value <node> <value>`, `paste <node> <text>` | Enter text |
| `key <node> <key>` | Press a key or chord on a node |
| `scroll <node> --dy N` | Scroll a scroller by points |
| `swipe up\|down\|left\|right --target <node>` | Touch swipe (mobile) |
| `long-press <node>`, `pinch --scale S` | Touch gestures (mobile) |
| `hover <node>`, `drag <node> --dx N --dy N` | Pointer (desktop, web) |
| `viewport --width W --height H` | Resize the window (desktop, web) |
| `system back\|home\|rotate\|…` | Platform actions (mobile) |
| `screenshot --inline --out shot.png` | PNG of the app, written on this machine |
| `attach`, `detach`, `sessions` | Save, remove or list a reusable address and token with `--session <name>` |

Run `gpui-agent <command> --help` for the options of each.

## Fewer calls and tokens

- Read part of the tree: `snapshot --depth 2` keeps two levels, and a node
  whose children were cut shows `children_count=N`. `snapshot --root <id|@ref>`
  keeps one subtree. Both work with `--refs` and the JSON output.
- Act and wait in one call: add `--wait-for <id>` (or `--wait-for-gone <id>`)
  to any action. The reply carries the new tree in compact form as `compact`;
  `--wait-timeout-ms` bounds the wait (default 5000).
- Refuse a stale target: `snapshot --fingerprints` lists each node's
  fingerprint; `click`, `type`, `set-value`, `key`, and `press` take
  `--expect-fingerprint <n>` and fail with `stale_target`, before acting, when
  the node changed since. `snapshot --refs` saves each ref's fingerprint, and
  `--guard` sends it only when one of those five commands' action target is an
  `@eN` ref. A fingerprint
  covers the node's value, so take a new snapshot after changing a node before
  guarding it again.
- Over MCP the same arguments are `depth`, `root`, `wait_for`,
  `wait_for_gone`, `wait_timeout_ms` and `expect_fingerprint`, and results are
  compact JSON.

## What a host supports

A host lists what it implements in `hello.features` (`gpui-agent hello`). An
action the host does not list fails with `unsupported_op`; that is a fact about
the platform (no hover on a phone, no swipe on desktop), not an error to retry.
Pick another action.

## Rules

- Read before acting: take a snapshot first, and again after every action that
  changes the screen.
- Prefer node ids or refs to coordinates. Coordinates are in the app's points,
  not screen pixels.
- An action that returns `ok` was delivered; check the next snapshot to see
  what it did.
- `screenshot` without `--inline` writes the file on the host's machine or
  device, which is not this one for Android or a remote host.
