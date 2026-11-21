//! Experimental recipes + TMP-inspired mapping for gpui-agent.
//!
//! This crate is **client-side**. It does not add a wire `batch` op and it
//! cannot bypass token / loopback / line-connection-mailbox caps. A recipe
//! is a compiled list of ordinary protocol [`Op`](gpui_agent::Op)s run
//! on one reused TCP session (`AgentClient` from P0). All-Read DAG waves
//! may pipeline (write N lines, then read). Write, Exit, mixed, and
//! `--screenshot-dir` waves stay one request at a time so a failed write
//! never continues the wave.

/// Recipe → scheduled plan compilation (DAG waves, param binding).
pub mod plan;
/// Deterministic `recipe run` receipt types.
pub mod receipt;
/// Recipe file format: parsing, params, validation.
pub mod recipe;
/// Operation schemas: protocol registry and user schema files.
pub mod registry;
/// Intent → wire-op resolution (`resolve_intent`).
pub mod resolve;
/// Plan execution: waves, screenshots, receipts.
pub mod run;
/// Schema types (`OpSchema`, `Effect`, `SchemaKind`).
pub mod schema;

pub use plan::{OrderCheck, Plan, PlannedStep, compile_plan, order_check};
pub use receipt::{Receipt, ScreenshotReceipt, StepReceipt};
pub use recipe::{
    MAX_RECIPE_STEPS, RECIPE_FORMAT_VERSION, Recipe, RecipeStep, apply_params, parse_recipe,
    parse_recipe_source, parse_wants, validate_recipe,
};
pub use registry::{
    Registry, load_schema_file, protocol_registry, registry_from_schema_paths, todo_registry,
};
pub use resolve::{ResolveResult, TokenFill, invoke_args, resolve_intent};
pub use run::{RunError, ScreenshotCapture, run_plan, run_plan_with_screenshots};
pub use schema::{ArgSchema, Effect, OpSchema, SchemaKind};
