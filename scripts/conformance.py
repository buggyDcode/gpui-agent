#!/usr/bin/env python3
"""Conformance checks every gpui-agent host must pass, whatever its platform.

Runs the `gpui-agent` CLI against one host (GPUI_AGENT_ADDR / GPUI_AGENT_TOKEN,
or --addr / --token passed through) and checks that:

- `hello` states the protocol, a platform and a feature list (a missing
  `framework`, which the protocol makes optional, is a warning);
- `snapshot` returns a non-empty tree with unique node ids;
- every capability-gated op the host does NOT list is refused with
  `unsupported_op`, and never acted on;
- the listed ops that are safe to try on any screen work: `scroll` and
  `swipe` on the first scroller (there and back), `hover` on the first button,
  `viewport` to the current size, and an inline screenshot that is a PNG.

`press` is only tried with --press <node id>, since it changes the app.
Exit code 0 when every check passed; 1 otherwise.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile

GATED = {
    "press": ["press", "--x", "1", "--y", "1"],
    "scroll": ["scroll", "--x", "1", "--y", "1", "--dy", "1"],
    "hover": ["hover", "--x", "1", "--y", "1"],
    "drag": ["drag", "--x", "1", "--y", "1", "--dx", "1"],
    "swipe": ["swipe", "up"],
    "long_press": ["long-press", "--x", "1", "--y", "1"],
    "pinch": ["pinch", "--scale", "1.1"],
    "viewport": ["viewport", "--width", "800", "--height", "600"],
    "system.back": ["system", "back"],
    "system.home": ["system", "home"],
    "system.rotate": ["system", "rotate", "landscape"],
}


class Run:
    def __init__(self, cli, passthrough):
        self.base = [cli] + passthrough
        self.failed = 0

    def cli(self, *args):
        proc = subprocess.run(self.base + list(args), capture_output=True, text=True)
        return proc.returncode, proc.stdout, proc.stderr.strip()

    def json(self, *args):
        code, out, err = self.cli(*args)
        if code != 0:
            return None, err
        try:
            return json.loads(out), ""
        except ValueError as exc:
            return None, f"not JSON: {exc}"

    def check(self, name, ok, detail=""):
        print(f"{'ok  ' if ok else 'FAIL'}  {name}{': ' + detail if detail else ''}")
        if not ok:
            self.failed += 1
        return ok

    def warn(self, text):
        print(f"warn  {text}")

    def skip(self, name, why):
        print(f"skip  {name}: {why}")


def walk(nodes):
    for node in nodes:
        yield node
        yield from walk(node.get("children", []))


def first(nodes, role):
    for node in walk(nodes):
        bounds = node.get("bounds") or {}
        if (
            node.get("role") == role
            and node.get("visible", True)
            and bounds.get("w", 0) > 0
            and bounds.get("h", 0) > 0
        ):
            return node["id"]
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--cli", default=os.environ.get("GPUI_AGENT_CLI", "gpui-agent"))
    parser.add_argument("--press", metavar="NODE", help="also press this node")
    args, passthrough = parser.parse_known_args()
    run = Run(args.cli, passthrough)

    reply, err = run.json("hello")
    hello = (reply or {}).get("hello")
    if not run.check("hello answers", hello is not None, err):
        return 1
    run.check("hello.protocol is 2", hello.get("protocol") == 2, str(hello.get("protocol")))
    run.check("hello.platform is stated", bool(hello.get("platform")))
    features = hello.get("features") or []
    current = "screenshot_inline" in features
    run.check("hello.features lists screenshot_inline", current, ", ".join(features))
    # Optional in the protocol, so its absence is reported and not failed.
    if hello.get("framework"):
        run.check("hello.framework is stated", True, hello["framework"])
    else:
        run.warn("hello.framework is not stated (optional)")

    reply, err = run.json("snapshot")
    nodes = ((reply or {}).get("tree") or {}).get("nodes") or []
    ids = [node["id"] for node in walk(nodes)]
    run.check("snapshot has nodes", bool(ids), err or f"{len(ids)} nodes")
    run.check("node ids are unique", len(ids) == len(set(ids)))

    for feature, command in GATED.items():
        if feature in features:
            continue
        code, _, err = run.cli(*command)
        run.check(
            f"{feature} is refused (not listed)",
            code != 0 and "unsupported_op" in err,
            "" if code != 0 else "the host acted on it",
        )

    scroller = first(nodes, "scroll")
    button = first(nodes, "button")
    for feature, there, back in (
        ("scroll", ["scroll", "{}", "--dy", "40"], ["scroll", "{}", "--dy", "-40"]),
        ("swipe", ["swipe", "up", "--target", "{}", "--distance", "40"],
         ["swipe", "down", "--target", "{}", "--distance", "40"]),
    ):
        if feature not in features:
            continue
        if scroller is None:
            run.skip(feature, "no visible scroller on this screen")
            continue
        for command in (there, back):
            code, _, err = run.cli(*[part.format(scroller) for part in command])
            run.check(f"{feature} {command[1] if feature == 'swipe' else command[3]} on {scroller}", code == 0, err)
    if "hover" in features:
        if button is None:
            run.skip("hover", "no visible button on this screen")
        else:
            code, _, err = run.cli("hover", button)
            run.check(f"hover {button}", code == 0, err)
    device = hello.get("device") or {}
    if "viewport" in features:
        if device.get("width") and device.get("height"):
            code, _, err = run.cli(
                "viewport", "--width", str(int(device["width"])), "--height", str(int(device["height"]))
            )
            run.check("viewport to the current size", code == 0, err)
        else:
            run.skip("viewport", "hello.device states no size")
    if args.press:
        code, _, err = run.cli("press", args.press)
        run.check(f"press {args.press}", code == 0, err)

    if current:
        with tempfile.TemporaryDirectory() as scratch:
            path = os.path.join(scratch, "shot.png")
            code, _, err = run.cli("screenshot", "--inline", "--out", path)
            png = code == 0 and open(path, "rb").read(8) == b"\x89PNG\r\n\x1a\n"
            unavailable = "screenshot_unavailable" in err
            if unavailable:
                run.skip("inline screenshot", "the host has no surface to capture")
            else:
                run.check("inline screenshot is a PNG on this machine", png, err)

    print(f"\n{'PASS' if run.failed == 0 else f'{run.failed} FAILED'}")
    return 1 if run.failed else 0


if __name__ == "__main__":
    sys.exit(main())
