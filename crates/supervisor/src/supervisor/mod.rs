//! The in-process supervisor (port of `packages/supervisor/src/supervisor.ts`).
//! The harness home is passed in, never read from the environment here, so tests in one process stay independent.
//!
//! - `events`: `ApiError`, `UiEvent` and the listener fan-out.
//! - `core`: settings, computers, runtime map and app-level reads.
//! - `workspaces`: create, delete, activate.
//! - `org`: SSO join, signed policy refresh, org locks.

mod app;
mod core;
mod events;
mod org;
mod service;
mod workspaces;

pub use core::{Core, CreateOptions, PolicyOverrides, Sso};
pub use events::{ApiError, Emitter, Listener, UiEvent};
pub use service::Supervisor;
