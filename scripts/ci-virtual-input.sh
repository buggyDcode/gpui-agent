#!/usr/bin/env bash
# Virtual delivery against the painted todo app (`--features embedded-host`).
#
# todo-headless has no window, so it cannot show whether a virtual click
# lands where the widget is painted or whether widget handlers can run
# without re-entering the app entity. This starts the real window on its own
# Xvfb display (Vulkan through whatever ICD the loader finds; CI points it at
# Mesa's lavapipe) and runs scripts/virtual_input.py against it.
#
# Needs Xvfb, a Vulkan ICD and a non-empty GPUI_AGENT_TOKEN.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ADDR="${GPUI_AGENT_ADDR:-127.0.0.1:17423}"
TOKEN="${GPUI_AGENT_TOKEN:-}"
export GPUI_AGENT=1
export GPUI_AGENT_ADDR="$ADDR"
export GPUI_AGENT_TOKEN="$TOKEN"

if [[ -z "$TOKEN" ]]; then
  echo "ci-virtual-input.sh requires a non-empty GPUI_AGENT_TOKEN" >&2
  exit 1
fi
if [[ "$ADDR" != 127.* ]]; then
  echo "refusing non-loopback GPUI_AGENT_ADDR=$ADDR" >&2
  exit 1
fi

echo "==> building CLI + todo (embedded-host)"
cargo build -p gpui-agent-cli -p todo --features todo/embedded-host

TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
CLI="$TARGET/debug/gpui-agent"
APP="$TARGET/debug/todo"
LOGS="$(mktemp -d)"

cleanup() {
  if [[ -n "${APP_PID:-}" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [[ -n "${XVFB_PID:-}" ]] && kill -0 "$XVFB_PID" 2>/dev/null; then
    kill "$XVFB_PID" 2>/dev/null || true
    wait "$XVFB_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "==> starting Xvfb"
# -displayfd picks a free display number and writes it once the server
# accepts connections.
Xvfb -displayfd 3 -screen 0 1280x800x24 -nolisten tcp 3>"$LOGS/display" 2>"$LOGS/xvfb.log" &
XVFB_PID=$!
for _ in $(seq 1 50); do
  [[ -s "$LOGS/display" ]] && break
  if ! kill -0 "$XVFB_PID" 2>/dev/null; then
    cat "$LOGS/xvfb.log" >&2
    exit 1
  fi
  sleep 0.1
done
if [[ ! -s "$LOGS/display" ]]; then
  echo "Xvfb did not report a display" >&2
  exit 1
fi
export DISPLAY=":$(head -n1 "$LOGS/display")"
# A Wayland socket in the environment would take precedence over DISPLAY.
unset WAYLAND_DISPLAY

echo "==> starting todo on $DISPLAY, agent on $ADDR"
"$APP" >"$LOGS/todo.log" 2>&1 &
APP_PID=$!

ready=0
for _ in $(seq 1 60); do
  if "$CLI" --addr "$ADDR" --reply-timeout-ms 1000 hello >/dev/null 2>&1; then
    ready=1
    break
  fi
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  echo "todo did not answer hello" >&2
  cat "$LOGS/todo.log" >&2
  exit 1
fi

status=0
python3 scripts/virtual_input.py --cli "$CLI" --addr "$ADDR" || status=$?
if ! kill -0 "$APP_PID" 2>/dev/null; then
  echo "todo exited during the checks" >&2
  status=1
fi
if [[ "$status" != 0 ]]; then
  echo "==> todo log" >&2
  cat "$LOGS/todo.log" >&2
fi
exit "$status"
