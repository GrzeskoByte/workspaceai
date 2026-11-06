//! The workspace supervisor, in-process: port of `packages/supervisor`. No GPUI, no async runtime.
//! `Supervisor::handle(method, path, query, body)` serves the same paths and JSON as the TS `routes.ts`.

pub mod jsonfile;
pub mod keychain;
pub mod paths;
pub mod policy;
mod router;
pub mod runtime;
pub mod supervisor;

pub use runtime::Runtime;
pub use supervisor::{ApiError, Core, Supervisor, UiEvent};
