# Performance notes

Session reuse, NDJSON
buffer reuse, tree flatten, mailbox `take`. Recipes reuse that session;
all-Read DAG waves may `rpc_pipeline` (write N lines, then read). Write /
Exit / mixed waves stay sequential.

Numbers: cloud VM, `x86_64`, `rustc 1.98.1`, Criterion `--quick`,
`cargo bench -p gpui-agent --bench agent_perf` (release). Median times
from this branch.

Security caps are unchanged: `MAX_LINE_BYTES = 1 MiB`,
`MAX_CONNECTIONS = 32`, `MAX_MAILBOX_DEPTH = 128`. No OS HID. No
`unsafe`.

```bash
cargo bench -p gpui-agent --bench agent_perf
```

## P0 measured (this branch)

| Bench | Median | Notes |
| --- | --- | --- |
| `rpc_once_reconnect_32_hellos` | **324 ms** | Historical per-op TCP connect |
| `rpc_session_reuse_32_hellos` | **536 µs** | **~605×** vs reconnect |
| `rpc_pipeline_32_hellos` | **145 µs** | **~3.7×** vs sequential session hellos |
| `handle_request_inprocess_32_hellos` | **1.88 µs** | CPU floor, no socket |
| `tree_flatten_naive_intermediate_vecs` | **5.29 µs** | Old per-child `Vec` (100-row tree) |
| `tree_flatten_capacity` | **1.06 µs** | **~5.0×** vs naive |
| `tree_flatten_into_reuse` | **994 ns** | `clear` keeps capacity |
| `mailbox_push_take_full` (128) | **22.4 µs** | `mem::take` |
| `snapshot_to_string` | **43.4 µs** | 100-row tree |
| `snapshot_write_json_line_reuse` | **43.4 µs** | Same ballpark; avoids the extra `String` |

Same shape as the experimental recipe branches (PR #4/#5). The headline
win is session reuse, not serialize.

## What P0 changed

- **`AgentClient::rpc`** keeps one TCP session. MCP already holds one
  client for the process, so `tools/call` no longer reconnects.
  **`rpc_once`** is the reconnect path for benches.
- **NDJSON helpers** (`write_json_line`, `read_limited_line_into`) reuse
  4 KiB encode/line buffers on client and server.
- **`UiTree::flatten_into`** visits once; no `node_count` pre-walk (that
  extra pass lost on #5).
- **Mailbox** stores `MailboxRequest` and `take`s with `mem::take`.
  Depth cap still 128.
- **todo `tree()`** pre-sizes the item list.

## Intentionally not in P0 / P1

| Technique | Where it was tried | Why it is not here |
| --- | --- | --- |
| `rpc_pipeline` writes | this branch / PR #5 | All-Read waves only. Write siblings stay fail-fast. Retry-after-write is Fatal (`4d464c7`). |
| simd-json | PR #5 | Hello parse **slower** than serde_json on tiny lines |
| tokio | PR #5 | Runtime build already ≈ one hello RTT |
| Scoped threads on the tree | PR #5 | 100-node count ~55× **worse** than sequential |
| Wire `batch` op | PR #5 | Protocol bump; pipeline of ordinary lines is enough later |
| mimalloc / compact_str / SmallVec / FxHashMap | PR #5 | JSON + loopback, not allocator- or hash-bound at todo size |

`UiNode` is **168 bytes** on 64-bit (`uinode_layout_stays_compact`).
`visible` fits in the existing bool padding before `Bounds`.

## Integrators

This workspace does not set `lto`. A product binary that embeds
`gpui-agent` may use thin LTO; expect a small serialize/parse win, not
another 600×. Measure with `cargo bench` on the real host tree size.

## M3 — snapshot revision cache (GATE-P)

`AgentHost::revision()` returning `Some(rev)` lets the server reuse the
last built `UiTree` while `rev` is unchanged. `wait_until` and `assert`
polls stop paying the rebuild entirely; mailbox `wait_until` posts a
cheap `revision` probe and only enqueues the assert after `rev` moves.
`None` (the default) keeps the old every-poll rebuild — always correct.

Measured: Apple Silicon, `cargo bench -p gpui-agent --bench agent_perf`,
1000-node tree (3001 nodes with children):

| Bench | Median | Notes |
| --- | --- | --- |
| `snapshot_rebuild_1000_nodes` | **436 µs** | Full `UiTree` build |
| `snapshot_cached_1000_nodes` | **10.5 ns** | Rev hit — **~41,000×** |
| `wait_until_poll_1000_nodes_uncached` | **450 µs** | Rebuild + assert per poll |
| `wait_until_poll_1000_nodes_cached` | **6.9 µs** | Assert only — **~64×** |
| `snapshot_compact_1000_nodes` | **179 µs** | Compact render (1 line/node) |

GATE-P verdicts:
- `wait_until` on an unchanged tree: **0 rebuilds** after the first
  (`wait_until_unchanged_tree_never_rebuilds` pins it — 45 ms of polling
  costs exactly one build).
- `snapshot {since}` at the current rev answers
  `{"unchanged":true}` — the whole response is **< 1 KiB**.
- `since` against a ring-cached rev answers a `diff` (`added` /
  `removed` / `changed`); past 8 revisions it degrades to the full tree
  with `{"full":true}` — never wrong, only bigger.

## M2–M8 — Instinct policy (GATE-R / GATE-L)

`gpui-agent-policy` is the local decision engine (`instinct` feature,
on in the CLI's default build; opt-in `jev` adds the System One
fallback). Everything below
is in [docs/bench/RESULTS.md](bench/RESULTS.md).

| Number | Value |
| --- | --- |
| `resolve` decision latency (GATE-R bar) | **p50 46 µs** ≤ 50 µs |
| Resolve accuracy vs baseline | R1 wrong=0, pass 59.2% vs R0 44.8% |
| `run --goal` local arm (GATE-L, A2) | **16/16, wrong=0** (T09 refuse correct) |
| Escalated decision | one subprocess spawn (M7) or one System One POST (M8) **per abstain only** — a local choice costs zero model calls |

Determinism: the local policy reads no clocks, RNG, env, or unordered
maps in the decision path — same tree + same goal → same decision.
Abstain never becomes a dispatch.

LLM-arm comparisons (A0/A1 GATE-T, A3 GATE-E, A4) are pending owner
API keys; the wire and validation paths are covered by scripted
endpoints, not live models.

## 0.3.0 re-validation (2026-11-21)

Re-run on tag `v0.3.0` on two machines: an Apple M4 Pro VM (macOS 26.6.2,
rustc 1.100.0) and an x86_64 Ubuntu 22.04 cloud VM (rustc 1.99.0).
Full Criterion runs plus the `gpui-agent-bench` arms at the debug
profile CI uses.

### Criterion (`cargo bench -p gpui-agent`)

| Bench | macOS (M4 Pro) | Linux (x86_64) |
| --- | --- | --- |
| `rpc_once_reconnect_32_hellos` | 516 ms | 325 ms |
| `rpc_session_reuse_32_hellos` | 1.79 ms | 1.64 ms |
| `rpc_pipeline_32_hellos` | 245 µs | 426 µs |
| `handle_request_inprocess_32_hellos` | 19.4 µs | 97.0 µs |
| `tree_flatten_capacity` | 714 ns | 1.33 µs |
| `tree_flatten_into_reuse` | 349 ns | 1.24 µs |
| `tree_flatten_naive_intermediate_vecs` | 5.85 µs | 6.75 µs |
| `snapshot_to_string` | 34.6 µs | 54.2 µs |
| `snapshot_write_json_line_reuse` | 33.6 µs | 52.6 µs |
| `snapshot_to_string_then_writeln` | 34.6 µs | 50.0 µs |
| `snapshot_rebuild_1000_nodes` | 278 µs | 613 µs |
| `snapshot_cached_1000_nodes` | 3.65 ns | 13.0 ns |
| `snapshot_compact_1000_nodes` | 130 µs | 134 µs |
| `wait_until_poll_1000_nodes_cached` | 4.06 µs | 6.65 µs |
| `wait_until_poll_1000_nodes_uncached` | 286 µs | 703 µs |
| `mailbox_push_take_full` | 8.66 µs | 27.2 µs |

Session reuse vs reconnect: ~289× on the M4 Pro, ~198× on this cloud VM.
The 605× in the P0 table was a different machine; the gap is machine
timing, not a wire change.

### `gpui-agent-bench` arms

| Arm | Pass | Wrong | Refused | p50 | p90 |
| --- | --- | --- | --- | --- | --- |
| R0 (keywords), Linux | 1120/2500 (44.8%) | 500 | 1240 | 78 µs | 89 µs |
| R1 (instinct), Linux | 1480/2500 (59.2%) | 0 | 1600 | 197 µs | 354 µs |
| R0, macOS | 1120/2500 (44.8%) | 500 | 1240 | 69 µs | 97 µs |
| R1, macOS | 1480/2500 (59.2%) | 0 | 1600 | 131 µs | 233 µs |
| A2 (`run --goal`), Linux | 48/48 | 0 | 9\* | 20.9 ms | 181 ms |
| A2, macOS | 48/48 | 0 | 9\* | 36.2 ms | 148 ms |

\* the task set includes refuse tasks; their refusals count as passes.

Release-profile arms on macOS: R0 p50 11 µs / p90 14 µs, R1 p50 28 µs /
p90 47 µs — under the M2 table's 20 µs / 25 µs and 46 µs / 75 µs. The
pass, wrong and refused counts match
[docs/bench/RESULTS.md](bench/RESULTS.md) exactly: the resolver and the
local policy are deterministic, so only latencies move between machines.
