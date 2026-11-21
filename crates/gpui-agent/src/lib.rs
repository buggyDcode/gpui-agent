//! GPUI Agent — embeddable SDK + protocol for controllable GPUI Kit apps.
//!
//! This is **not** Chrome `DevTools` Protocol. GPUI Kit apps are GPU-rendered
//! native surfaces with no DOM, so Playwright/CDP cannot attach. Agents talk
//! to an opt-in NDJSON server that publishes a semantic UI tree
//! (stable ids, roles, names, state) and dispatches actions into the same
//! handlers the widgets use.
//!
//! **SDK entry:** [`prelude`], [`testing::TestHost`], [`UiNode`] constructors
//! in [`tree::role`]. Cookbook: `docs/SDK.md`. Product agent-machine
//! mutations go through a **daemon** (`docs/ADR-001-daemon-sot.md` in the
//! repo); in-process [`AgentHost`] is for widget E2E and embedders.
//!
//! Platform seams (`PlatformKind`) keep the protocol stable across desktop,
//! a headless test host, and later web (GPUI WASM) or mobile hosts.

/// Chunked blob transfer for oversized results (screenshots).
pub mod blob;
/// The blocking client SDK.
pub mod client;
/// Id-keyed diffs between two snapshots.
pub mod diff;
/// Running-host discovery (`targets`).
pub mod discovery;
/// Request → response dispatch shared by every transport.
pub mod dispatch;
/// Stable error-code classification.
pub mod error_code;
/// Bounded host event stream.
pub mod event;
/// Challenge/response auth (HMAC-SHA256 over a nonce).
pub mod hmac_auth;
/// The platform-side host trait.
pub mod host;
/// Action-id keymap dispatch.
pub mod keybinding;
/// Cross-thread inbox for posting work onto a UI thread.
pub mod mailbox;
/// One-JSON-value-per-line I/O helpers.
pub mod ndjson;
/// Client-side subtree/depth cuts of a snapshot.
pub mod outline;
/// The `use gpui_agent::prelude::*` bundle.
pub mod prelude;
/// The v2 wire protocol types.
pub mod protocol;
/// Observe-only pixel capture of the app surface.
pub mod screenshot;
/// Scrolled-capture planning and stitching.
pub mod scroll_capture;
/// The `GPUI_AGENT*` opt-in gate.
pub mod security;
/// The per-connection TCP server.
pub mod server;
/// In-process test-host helpers for SDK E2E.
pub mod testing;
/// The semantic UI tree.
pub mod tree;
/// Virtual (in-process GPUI event) input planning.
pub mod virtual_input;

pub use client::{AgentClient, WaitFor};
pub use diff::{AddedNode, ChangedNode, TreeDiff, apply_diff, compact_lines, diff_trees};
pub use discovery::{
    ClientTarget, HostRecord, ResolveError, live_hosts, resolve_client_addr, resolve_client_target,
    runtime_root,
};
pub use dispatch::{
    DispatchResult, TreeCache, assert_tree, authorize_request, check_fingerprint_gate,
    handle_request, handle_request_cached,
};
pub use error_code::{CallFailure, DeliveryStatus, error_code_of, hint_for};
pub use hmac_auth::{
    Challenge, challenge_for_nonce, hex_decode, hex_encode, hmac_hex, hmac_verify,
};
pub use host::AgentHost;
pub use keybinding::{
    KEYBINDING_UNAVAILABLE, KeybindingInfo, KeybindingRequest, authorize_keybinding,
    authorize_keybinding_op, binding_is_dangerous, complete_keybinding_action,
    intercept_keybinding_action, is_keybinding_unavailable, is_quit_binding, is_quit_chord,
    keybinding_list_json, keybinding_unavailable, op_is_confirmed_quit,
};
pub use mailbox::{AgentMailbox, MAX_MAILBOX_DEPTH, MailboxRequest};
pub use ndjson::{line_is_blank, read_limited_line, read_limited_line_into, write_json_line};
pub use outline::{Outline, OutlineNode, OutlineOptions, outline};
pub use protocol::{
    AssertSpec, DeliveryMode, DeviceInfo, HelloAuth, HelloInfo, KeybindingScope, Op,
    PROTOCOL_VERSION, PlatformKind, ReadBack, Request, Response, ScreenshotMode, SnapshotFormat,
    SwipeDirection, SystemAction, UNSUPPORTED_OP, is_unsupported_op, unsupported_op,
};
pub use screenshot::{
    SCREENSHOT_BACKEND_SCREENCAPTURE, SCREENSHOT_UNAVAILABLE, TEST_PNG, accept_written_png,
    atomic_write_png, capture_window_png_bytes, capture_window_via_screencapture,
    confine_screenshot_path, confine_screenshot_path_in, is_screenshot_unavailable,
    png_write_temp_path, require_screenshot_path, screencapture_window_argv, screenshot_base_dir,
    screenshot_unavailable, write_png, write_png_in,
};
pub use scroll_capture::{
    DEFAULT_MAX_HEIGHT_PX, MAX_SCROLL_TILES, MAX_SCROLLED_PNG_BYTES, RgbaImage, SCROLL_UNAVAILABLE,
    ScreenshotSpec, ScrollMetrics, TileSpec, crop_window_png, decode_png_rgba, encode_png_rgba,
    is_scroll_unavailable, plan_scroll_tiles, run_scrolled_capture_sync, scroll_unavailable,
    scrolled_dispatch_result, stitch_tiles_vertically,
};
pub use security::{
    AgentConfig, INSECURE_NO_TOKEN_BANNER, SecurityError, authorize_bind,
    authorize_bind_with_insecure, authorize_client, ensure_loopback, from_env, is_loopback_addr,
    tokens_match,
};
pub use server::{
    AgentServer, DEFAULT_ADDR_STR, DEFAULT_PORT, MAX_CONNECTIONS, MAX_LINE_BYTES, ServerLimits,
    default_addr,
};
pub use testing::TestHost;
pub use tree::{
    Bounds, IN_VIEWPORT_UNAVAILABLE, SECRET_MARKERS, SECRET_MASK, UiNode, UiTree,
    in_viewport_unavailable, is_in_viewport_unavailable, role,
};
pub use virtual_input::{
    AgentCursor, VIRTUAL_UNAVAILABLE, VirtualPointerClick, hit_point, keystroke_token, plan_click,
    text_keystrokes, virtual_unavailable,
};

/// Parse `"{prefix}{n}"` into `n`. Apps use this for numbered stable ids
/// (`row-3`, `tab-1`); prefixes themselves are app-defined.
pub fn parse_numbered_id(prefix: &str, target: &str) -> Option<u64> {
    target
        .strip_prefix(prefix)
        .and_then(|rest| rest.parse().ok())
}

#[cfg(test)]
mod parse_id_tests {
    use super::parse_numbered_id;

    #[test]
    fn parses_app_defined_prefixes() {
        assert_eq!(parse_numbered_id("row-", "row-3"), Some(3));
        assert_eq!(parse_numbered_id("tab-", "tab-1"), Some(1));
        assert_eq!(parse_numbered_id("row-", "other-3"), None);
        assert_eq!(parse_numbered_id("row-", "row-x"), None);
    }
}

#[cfg(test)]
mod cap_tests {
    use super::{MAX_CONNECTIONS, MAX_LINE_BYTES, MAX_MAILBOX_DEPTH};

    #[test]
    fn security_caps_unchanged() {
        assert_eq!(MAX_LINE_BYTES, 1024 * 1024);
        assert_eq!(MAX_CONNECTIONS, 32);
        assert_eq!(MAX_MAILBOX_DEPTH, 128);
    }
}
