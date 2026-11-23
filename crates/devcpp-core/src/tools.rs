//! Tool Manager.
//!
//! Dev-C++ lets the user register external programs ("Tools") that can be
//! invoked from the IDE (`Source/ToolFrm.pas`, `Source/ToolEditFrm.pas`). The
//! Linux clone keeps the same model: a named command with placeholders that
//! are expanded before execution.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// A configured external tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Name shown in the menu.
    pub name: String,
    /// Executable to run.
    pub command: String,
    /// Command line arguments; supports the placeholders below.
    pub args: String,
    /// Working directory; empty means the project directory.
    pub working_dir: String,
    /// Whether output is shown in a console window.
    pub show_console: bool,
}

impl ToolEntry {
    /// Expands the placeholders in `args`.
    ///
    /// Supported placeholders:
    /// * `%PROJECT%` — directory of the current project
    /// * `%PROJECTFILE%` — path of the `.dev` file
    /// * `%COMPILER%` — C compiler
    /// * `%CPPCOMPILER%` — C++ compiler
    /// * `%DEBUGGER%` — GDB
    /// * `%FILE%` — path of the active file
    #[must_use]
    pub fn expand_args(&self, ctx: &ToolContext) -> String {
        let mut out = self.args.clone();
        for (key, value) in [
            ("%PROJECT%", ctx.project_dir.to_string_lossy().into_owned()),
            (
                "%PROJECTFILE%",
                ctx.project_file.to_string_lossy().into_owned(),
            ),
            ("%COMPILER%", ctx.compiler.clone()),
            ("%CPPCOMPILER%", ctx.cpp_compiler.clone()),
            ("%DEBUGGER%", ctx.debugger.clone()),
            ("%FILE%", ctx.active_file.to_string_lossy().into_owned()),
        ] {
            out = out.replace(key, &value);
        }
        out
    }
}

/// Values available for placeholder expansion.
#[derive(Debug, Clone, Default)]
pub struct ToolContext {
    /// Directory of the current project.
    pub project_dir: PathBuf,
    /// Path of the `.dev` file.
    pub project_file: PathBuf,
    /// Active file in the editor.
    pub active_file: PathBuf,
    /// C compiler command.
    pub compiler: String,
    /// C++ compiler command.
    pub cpp_compiler: String,
    /// Debugger command.
    pub debugger: String,
}

/// The registry of external tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolManager {
    /// Registered tools.
    pub tools: Vec<ToolEntry>,
}

impl ToolManager {
    /// Creates an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a tool.
    pub fn add(&mut self, tool: ToolEntry) {
        self.tools.push(tool);
    }

    /// Removes a tool by name.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.tools.len();
        self.tools.retain(|t| t.name != name);
        self.tools.len() != before
    }

    /// Finds a tool by name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&ToolEntry> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// Runs a tool and captures its output.
    ///
    /// # Errors
    /// Returns an error when the tool cannot be started.
    pub fn run(&self, name: &str, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let tool = self
            .find(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        let args = tool.expand_args(ctx);
        let cwd: PathBuf = if tool.working_dir.is_empty() {
            ctx.project_dir.clone()
        } else {
            Path::new(&tool.working_dir).to_path_buf()
        };

        let mut cmd = Command::new(&tool.command);
        for arg in args.split_whitespace() {
            cmd.arg(arg);
        }
        if !cwd.as_os_str().is_empty() {
            cmd.current_dir(&cwd);
        }
        let output = cmd.output().map_err(|e| ToolError::Spawn(e.to_string()))?;
        Ok(ToolOutput {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            success: output.status.success(),
        })
    }
}

/// Result of running an external tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// Exit status success flag.
    pub success: bool,
}

/// Errors of the tool manager.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// No tool with that name is registered.
    #[error("tool not found: {0}")]
    NotFound(String),
    /// The tool could not be started.
    #[error("cannot start tool: {0}")]
    Spawn(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn expands_placeholders() {
        let tool = ToolEntry {
            name: "Compile".to_string(),
            command: "gcc".to_string(),
            args: "-c %PROJECT%/main.cpp -I%PROJECT%".to_string(),
            working_dir: String::new(),
            show_console: false,
        };
        let ctx = ToolContext {
            project_dir: PathBuf::from("/proj"),
            ..ToolContext::default()
        };
        assert_eq!(tool.expand_args(&ctx), "-c /proj/main.cpp -I/proj");
    }

    #[test]
    fn runs_tool_and_captures_output() {
        let mut manager = ToolManager::new();
        manager.add(ToolEntry {
            name: "Echo".to_string(),
            command: "echo".to_string(),
            args: "hello".to_string(),
            working_dir: String::new(),
            show_console: false,
        });
        let out = manager
            .run("Echo", &ToolContext::default())
            .expect("run tool");
        assert!(out.success);
        assert_eq!(out.stdout.trim(), "hello");
    }

    #[test]
    fn unknown_tool_is_error() {
        let manager = ToolManager::new();
        assert!(manager.run("nope", &ToolContext::default()).is_err());
    }

    #[test]
    fn removes_tool() {
        let mut manager = ToolManager::new();
        manager.add(ToolEntry {
            name: "X".to_string(),
            command: "true".to_string(),
            args: String::new(),
            working_dir: String::new(),
            show_console: false,
        });
        assert!(manager.remove("X"));
        assert!(!manager.remove("X"));
    }
}
