//! The opencode engine: detection, the HTTP client, the managed `opencode serve` process and remote mode.
//! Port of `opencode.ts` and `detect.ts`. Blocking ureq plus std threads only — no async runtime.
//!
//! - `remote`: `remoteUrlAllowed` (R-35) and `testRemote`.
//! - `client`: `OpencodeClient`, the JSON HTTP client (and an SSE reader for batch 5).
//! - `detect`: installed binary lookup, pinned releases, `opencodeBinary`.
//! - `server`: `ManagedServer` (`opencode serve`, health poll, logs, stop, idle watchdog).
//! - `lifecycle`: the `Runtime` engine methods (start, restart, stop, env, status).

pub mod client;
pub mod detect;
pub(crate) mod lifecycle;
pub mod remote;
pub mod server;

pub use client::{OpencodeClient, OpencodeError};
pub use detect::{opencode_binary, pinned_opencode_bin, pinned_opencode_dir};
pub use remote::{remote_url_allowed, test_remote};
pub use server::{free_port, ManagedOptions, ManagedServer};
