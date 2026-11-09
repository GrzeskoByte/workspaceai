//! The workspace supervisor, in-process: port of `packages/supervisor`. No GPUI, no async runtime.
//! `Supervisor::handle(method, path, query, body)` serves the same paths and JSON as the TS `routes.ts`.

pub mod agents;
pub mod engine;
pub mod export;
pub mod files;
pub mod formats;
pub mod headless;
pub mod jsonfile;
pub mod keychain;
pub mod paths;
pub mod policy;
pub mod projects;
pub mod providers;
pub mod recall;
mod router;
pub mod runtime;
pub mod secrets;
pub mod sessions;
pub mod supervisor;

pub use runtime::Runtime;
pub use supervisor::{ApiError, Core, Supervisor, UiEvent};
