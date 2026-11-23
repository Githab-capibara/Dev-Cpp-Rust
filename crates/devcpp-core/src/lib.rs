//! devcpp-core — the headless brain of the devcpp IDE.
//!
//! Everything that is not GTK lives here: the `.dev` project format, Makefile
//! generation, toolchain detection, the build/run runner, compiler diagnostics,
//! the GDB/MI debugger client, the project symbol index and code-completion
//! model, project templates, the tool manager and user configuration.
//!
//! The crate is deliberately GUI-free so that the entire IDE logic can be unit
//! tested without a display server.

pub mod build;
pub mod completion;
pub mod config;
pub mod debugger;
pub mod diagnostics;
pub mod makefile;
pub mod mi;
pub mod paths;
pub mod project;
pub mod symbols;
pub mod templates;
pub mod toolchain;
pub mod tools;

pub use project::Project;
pub use project::ProjectType;
