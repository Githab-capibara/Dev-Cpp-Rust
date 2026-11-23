//! Toolchain detection: finds GCC/Clang, GDB, make and formatters on `PATH`.
//!
//! Dev-C++ ships its own TDM-GCC on Windows. On Linux the IDE uses the system
//! compiler, exactly like a hand-written Makefile would.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A detected external tool.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    /// Human readable name, e.g. "C Compiler".
    pub name: String,
    /// Executable name as stored in the project file.
    pub command: String,
    /// Resolved absolute path, if found.
    pub path: Option<PathBuf>,
    /// Version string reported by the tool.
    pub version: Option<String>,
}

impl Tool {
    /// Whether the tool was found on `PATH`.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.path.is_some()
    }
}

/// The complete set of tools used by the IDE.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toolchain {
    /// C compiler (`gcc`).
    pub c_compiler: Tool,
    /// C++ compiler (`g++`).
    pub cpp_compiler: Tool,
    /// Linker driver (`g++`).
    pub linker: Tool,
    /// Debugger (`gdb`).
    pub debugger: Tool,
    /// Build tool (`make`).
    pub make: Tool,
    /// Source formatter (`astyle`).
    pub astyle: Tool,
    /// Alternative formatter (`clang-format`).
    pub clang_format: Tool,
    /// Profiler data collector (`gprof`).
    pub gprof: Tool,
    /// GNU Make used for custom makefiles.
    pub gmake: Tool,
}

impl Toolchain {
    /// Detects all tools on the current `PATH`.
    #[must_use]
    pub fn detect() -> Self {
        Self {
            c_compiler: detect("C Compiler", "gcc"),
            cpp_compiler: detect("C++ Compiler", "g++"),
            linker: detect("Linker", "g++"),
            debugger: detect("Debugger", "gdb"),
            make: detect("Build Tool", "make"),
            astyle: detect("Code Formatter", "astyle"),
            clang_format: detect("Clang Formatter", "clang-format"),
            gprof: detect("Profiler", "gprof"),
            gmake: detect("GNU Make", "gmake"),
        }
    }

    /// Whether the minimum set required to build a C/C++ project is present.
    #[must_use]
    pub fn can_build(&self) -> bool {
        self.c_compiler.is_available()
            && self.cpp_compiler.is_available()
            && self.make.is_available()
    }

    /// Whether debugging is possible.
    #[must_use]
    pub fn can_debug(&self) -> bool {
        self.debugger.is_available()
    }
}

/// Finds `command` on `PATH` and captures its version string.
#[must_use]
pub fn detect(name: &str, command: &str) -> Tool {
    let path = which::which(command).ok();
    let version = path.as_ref().and_then(|p| probe_version(p));
    Tool {
        name: name.to_string(),
        command: command.to_string(),
        path,
        version,
    }
}

fn probe_version(path: &std::path::Path) -> Option<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_gcc_on_this_machine() {
        let tc = Toolchain::detect();
        assert!(tc.c_compiler.is_available(), "gcc must be on PATH");
        assert!(tc
            .c_compiler
            .version
            .as_deref()
            .unwrap_or_default()
            .contains("gcc"));
    }

    #[test]
    fn detects_make_and_gdb() {
        let tc = Toolchain::detect();
        assert!(tc.make.is_available());
        assert!(tc.debugger.is_available());
    }

    #[test]
    fn missing_tool_is_not_available() {
        let tool = detect("Nope", "definitely-not-a-real-tool-xyz");
        assert!(!tool.is_available());
        assert!(tool.version.is_none());
    }
}
