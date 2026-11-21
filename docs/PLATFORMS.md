# Platform support

What each host can do over protocol v2, measured by running
`scripts/conformance.py` and direct `click` / `type` / `screenshot`
commands against live hosts on tag `v0.3.0` (and `devin` builds of
the fixes landing in 0.3.1).

Matrix legend: **PASS** = conformance suite exits 0 (warnings listed);
each row also records what hands-on actions verified.

## Summary

| Host | Platform | Conformance | click | type into text field | screenshot (`--inline`) | discovery (`targets`) |
| --- | --- | --- | --- | --- | --- | --- |
| nsh AppKit gallery | desktop / macos | PASS | ok | **fails: `occluded_by`** | ok (PNG) | — |
| nsh Win32 gallery | desktop / windows | PASS (all ok) | ok | ok | ok (PNG) | — |
| nsh GTK gallery | desktop / linux | PASS (all ok) | ok | ok | ok (PNG) | — |
| nsh web | web | *see below* | — | — | — | — |
| nsh iOS gallery | mobile / ios (Simulator) | PASS | ok, verified | ok, verified | ok (PNG) | **was missing → fixed in 0.3.1** |
| nsh Android gallery | mobile / android | covered by nightly CI (`android-conformance.yml`) | — | — | — | `targets` lists adb devices + forwards |
| GPUI todo (`embedded-host`) | desktop / macos, windows | PASS (2 warn) | ok, verified | ok, verified | macOS: fixed in 0.3.1; other OSes `screenshot_unavailable` | ok |
| todo-headless | headless / any | PASS (1 warn, 1 skip) | ok | ok, verified | skip — no pixel surface | ok |

## nsh macOS (AppKit) — validated on macOS 26.6.2, Apple M4 Pro

`nsh run examples/gallery --target macos` with `GPUI_AGENT=1`. Hello:
protocol 2, `framework: nsh`, `deliveries: [semantic]`, features
`press, scroll, hover, drag, viewport, snapshot_since, snapshot_compact,
expect_fingerprint, secret_redaction, screenshot_inline`. 275-node tree.

- Conformance: **PASS** (every check ok, screenshot verified as a real PNG).
- `type`, `set-value`, `press` on text fields fail with
  `occluded_by: <ancestor id>` — the AppKit hit test at the field's
  center returns an ancestor container. **Known bug in nsh-platform-macos,
  not in gpui-agent**; tracked as an nsh PR into `buwiz`.
- `click` on some controls replies `delivered_unverified` while taking
  effect (navigation clicks verified via a follow-up snapshot).

## nsh iOS (Simulator) — validated on iPhone 17 simulator, iOS 27.0

`nsh run examples/gallery --target ios`. Hello: `platform: mobile`,
`os: ios`, `executable: nsh-ios-host`, features add `swipe, long_press,
system.back`; 713-node tree.

- Conformance: **PASS**, including `scroll`/`swipe` on real targets.
- `type name abc` verified (`after.value: "Abc"` — the field capitalizes;
  delivery verified).
- `screenshot --inline` returns a 1206×2622 PNG; `--out` writes under the
  app container's `Library/Caches/gpui-agent/`.
- **Fixed for 0.3.1:** sandboxed hosts publish their `HostRecord` inside
  the app container's `tmp`, which `targets` did not scan. 0.3.1 scans
  both the device tmp and every app-container tmp.

## nsh Windows (Win32) — validated on Windows Server 2022, 21H2

`nsh run examples/gallery --target windows`. Hello: `platform: desktop`,
`os: windows`, `framework: nsh`; 600-node tree.

- Conformance: **PASS**, all checks ok including scroll/hover/viewport.
- `type` into "Search demos" verified (`value: "Chat"`, list filtered).
- `screenshot --inline` returns an 1184×792 PNG.
- Caveat: gallery checked in light mode only.

## nsh Linux (GTK 4 ≥ 4.14)

`nsh run examples/gallery --target linux` under Xvfb on Ubuntu 24.04,
validated 2026-11-21 at nsh buwiz c218676 + gpui-agent 0.3.1 dev
(`devin/1795252272-mac-fixes`).

- `hello`: protocol 2, app "Native Shell", platform desktop, os linux,
  framework nsh, executable nsh-linux-host, auth required, deliveries
  [semantic]. Features: press, scroll, hover, drag, viewport,
  snapshot_since, snapshot_compact, expect_fingerprint, secret_redaction,
  screenshot_inline.
- `scripts/conformance.py`: all 33 checks pass — refusals on the 13
  unlisted ops (incl. `system.*`), `not_found`/`target_gone`/
  `not_delivered`/`stale_target` envelopes, scroll-and-back on the
  gallery scroller, hover, viewport, and a complete PNG via
  `screenshot --inline` (221-node snapshot).
- `click` and `type` verified through the conformance input checks on
  the gallery's interactive nodes.

## nsh web

The web host serves the gallery in a browser; the agent wire is the same
NDJSON protocol. The 0.3.1 validation did not re-run conformance here;
earlier runs passed. When re-run, drive it from this checkout:

```sh
nsh run examples/gallery --target web   # serves the wasm host + dev page
python3 scripts/conformance.py --addr <published addr>
```

## nsh Android — covered by nightly CI

`.github/workflows/android-conformance.yml` boots an Android emulator
on `ubuntu-latest`, runs `nsh run --target android` against the nsh
gallery (hexuria/nsh `buwiz` branch), forwards the host port over
`adb forward`, and runs `scripts/conformance.py` with
`--require-screenshot`. `gpui-agent targets` lists adb devices and their
forwarded ports so the CLI finds the host without a hard-coded address.

## GPUI todo demo (`embedded-host`) — macOS 26.6.2 and Windows 21H2

`cargo run -p todo --features embedded-host`. Hello: protocol 2,
`app: todo`, `deliveries: [semantic, virtual]`, `framework` unstated
(optional warn); 83-node tree.

- Conformance: **PASS** on both OSes (2 warns: `framework` optional,
  inline screenshot declined).
- `scripts/virtual_input.py`: all checks passed on both OSes — virtual
  clicks and typing drive the painted widgets, wrong-token input refused.
- **Fixed for 0.3.1:** inline screenshot on macOS failed because
  `screencapture` refuses dot-prefixed `.tmp` output names; the temp
  path now keeps a `.png` extension (`png_write_temp_path`).
- On Windows and Linux the app has no production framebuffer export, so
  `screenshot` answers `screenshot_unavailable` by design (honest error,
  not a fake image). macOS captures the app window via `screencapture -l`.

## todo-headless — Linux and Windows

`cargo run -p todo-headless -- serve`. Hello: `platform: headless`,
`deliveries: [semantic]`; 12-node tree.

- Conformance: **PASS** (warn: `framework` unstated; skip: no pixel
  surface — the suite skips inline screenshot on headless hosts).
- `type` verified on both OSes; `click` replies
  `delivered_unverified` because headless actions are mailboxed to the
  store rather than painted widgets.

## What is not covered

- Physical iOS/Android devices: only simulators and CI emulators were
  exercised; adb-forwarded physical Android devices work the same way
  but were not re-run for 0.3.1.
- `virtual` delivery exists only on the GPUI todo host; nsh hosts
  deliver `semantic` only.
- LLM bench arms (A0/A1/A3/A4) need owner API keys and are not part of
  platform validation; see [PERF.md](PERF.md).
