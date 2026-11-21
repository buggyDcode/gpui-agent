#!/usr/bin/env bash
# Runs scripts/conformance.py against an nsh app on a booted Android
# emulator: the nsh gallery under `nsh run --target android`, with its
# GPUI agent on, reached through `adb forward`.
#
# The emulator, the nsh CLI and its x86_64 development runtime are set up
# before (see .github/workflows/android-conformance.yml):
#
#   NSH_DIR       an nsh checkout with target/debug/nsh built
#   NSH_RUNTIME   a runtime root holding android/dev (classes.dex, lib/x86_64)
#   GPUI_AGENT_CLI  this repository's gpui-agent binary
#   ARTIFACTS     where logs and the last screen go (optional)
#
# The token is made here, masked in the Actions log, handed to `nsh run`
# (which stores it in the development host's private files) and to the CLI
# through the environment only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
: "${NSH_DIR:?NSH_DIR must point to an nsh checkout}"
: "${NSH_RUNTIME:?NSH_RUNTIME must point to the prebuilt Android runtime root}"
CLI="${GPUI_AGENT_CLI:-$ROOT/target/debug/gpui-agent}"
ARTIFACTS="${ARTIFACTS:-$ROOT/target/android-conformance}"
ADDR="${GPUI_AGENT_ADDR:-127.0.0.1:17421}"
PORT="${ADDR##*:}"
APP="${NSH_APP:-examples/gallery}"
START_TIMEOUT_S="${START_TIMEOUT_S:-900}"
mkdir -p "$ARTIFACTS"

if [[ "$ADDR" != 127.* ]]; then
  echo "refusing non-loopback GPUI_AGENT_ADDR=$ADDR" >&2
  exit 1
fi

token="$(python3 -c 'import secrets; print(secrets.token_hex(32))')"
if [[ -n "${GITHUB_ACTIONS:-}" ]]; then
  echo "::add-mask::$token"
fi
export GPUI_AGENT=1
export GPUI_AGENT_ADDR="$ADDR"
export GPUI_AGENT_TOKEN="$token"
# A busy emulator answers slowly; the default 8 s is too short.
export GPUI_AGENT_REPLY_TIMEOUT_MS="${GPUI_AGENT_REPLY_TIMEOUT_MS:-60000}"

NSH_PID=""
cleanup() {
  status=$?
  adb exec-out screencap -p > "$ARTIFACTS/screen.png" 2>/dev/null || true
  adb logcat -d > "$ARTIFACTS/logcat.txt" 2>/dev/null || true
  if [[ -n "$NSH_PID" ]] && kill -0 "$NSH_PID" 2>/dev/null; then
    kill -TERM -- "-$NSH_PID" 2>/dev/null || kill -TERM "$NSH_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$NSH_PID" 2>/dev/null || break
      sleep 0.5
    done
    kill -KILL -- "-$NSH_PID" 2>/dev/null || true
  fi
  adb forward --remove "tcp:$PORT" 2>/dev/null || true
  if [[ $status -ne 0 ]]; then
    echo "--- nsh run log (tail) ---" >&2
    tail -n 200 "$ARTIFACTS/nsh-run.log" >&2 || true
  fi
  exit "$status"
}
trap cleanup EXIT

adb wait-for-device
adb devices -l
adb shell getprop ro.product.cpu.abilist

echo "==> nsh run --target android $APP"
(
  cd "$NSH_DIR"
  exec setsid ./target/debug/nsh run --target android "$APP"
) < /dev/null > "$ARTIFACTS/nsh-run.log" 2>&1 &
NSH_PID=$!

adb forward "tcp:$PORT" "tcp:$PORT"
adb forward --list

echo "==> waiting for hello on $ADDR (up to ${START_TIMEOUT_S}s)"
deadline=$((SECONDS + START_TIMEOUT_S))
until "$CLI" --addr "$ADDR" --reply-timeout-ms 5000 hello > "$ARTIFACTS/hello.json" 2>/dev/null; do
  if ! kill -0 "$NSH_PID" 2>/dev/null; then
    echo "nsh run exited before the agent answered" >&2
    exit 1
  fi
  if (( SECONDS >= deadline )); then
    echo "the agent did not answer hello within ${START_TIMEOUT_S}s" >&2
    exit 1
  fi
  sleep 5
done
cat "$ARTIFACTS/hello.json"

echo "==> wait until ready"
"$CLI" --addr "$ADDR" wait --timeout-ms 120000

echo "==> conformance"
python3 "$ROOT/scripts/conformance.py" --cli "$CLI" --addr "$ADDR" | tee "$ARTIFACTS/conformance.txt"
