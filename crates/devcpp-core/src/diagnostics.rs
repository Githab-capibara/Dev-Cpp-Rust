//! Compiler diagnostics: parses GCC/Clang output into structured messages.
//!
//! GCC emits lines like:
//!
//! ```text
//! main.cpp:5:10: error: expected ';' before '}' token
//! main.cpp: In function 'int main()':
//! /usr/include/stdio.h:30:10: note: candidate ...
//! ```
//!
//! Dev-C++ parses the same output to build its "Compiler Output" panel and to
//! jump to the offending line (`TCompiler.ProcessOutput`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Fatal error, compilation stops.
    Error,
    /// Warning, compilation continues.
    Warning,
    /// Informational note.
    Note,
    /// Informational message.
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Info => "info",
        };
        f.write_str(text)
    }
}

/// A single compiler message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// File the message refers to, if any.
    pub file: Option<PathBuf>,
    /// 1-based line number.
    pub line: Option<u32>,
    /// 1-based column number.
    pub column: Option<u32>,
    /// Severity.
    pub severity: Severity,
    /// Message text without the location prefix.
    pub message: String,
    /// Raw source line the compiler quoted.
    pub source_line: Option<String>,
}

impl Diagnostic {
    /// Location formatted as `file:line:column`, for status bar display.
    #[must_use]
    pub fn location(&self) -> Option<String> {
        let file = self.file.as_ref()?;
        let mut s = file.display().to_string();
        if let Some(line) = self.line {
            let _ = std::fmt::Write::write_fmt(&mut s, format_args!(":{line}"));
            if let Some(col) = self.column {
                let _ = std::fmt::Write::write_fmt(&mut s, format_args!(":{col}"));
            }
        }
        Some(s)
    }
}

/// Parses compiler output into diagnostics.
///
/// Lines that are not diagnostics are returned separately so the GUI can show
/// the raw log.
#[must_use]
pub fn parse(output: &str) -> (Vec<Diagnostic>, Vec<String>) {
    let mut diagnostics = Vec::new();
    let mut raw = Vec::new();
    let mut pending_source: Option<String> = None;

    for line in output.lines() {
        if let Some(mut diag) = parse_line(line) {
            // GCC prints the offending source line *before* the message, so a
            // pending source line belongs to this diagnostic.
            if let Some(src) = pending_source.take() {
                diag.source_line = Some(src);
            }
            diagnostics.push(diag);
        } else {
            let trimmed = line.trim_end();
            // GCC prints the offending source line as "  12 |     code".
            if let Some(idx) = trimmed.find('|') {
                let before = trimmed[..idx].trim();
                if !before.is_empty() && before.chars().all(|c| c.is_ascii_digit()) {
                    pending_source = Some(trimmed[idx + 1..].trim_start().to_string());
                    continue;
                }
            }
            pending_source = None;
            if !trimmed.trim().is_empty() {
                raw.push(trimmed.to_string());
            }
        }
    }
    (diagnostics, raw)
}

/// Parses a single output line, returning `None` when it is not a diagnostic.
#[must_use]
pub fn parse_line(line: &str) -> Option<Diagnostic> {
    let trimmed = line.trim_start();
    // "<file>:<line>[:<col>]: <severity>: <message>"
    let (file, line_no, column, rest) = split_location(trimmed)?;
    let (severity, message) = split_severity(rest)?;
    Some(Diagnostic {
        file: file.map(PathBuf::from),
        line: line_no,
        column,
        severity,
        message,
        source_line: None,
    })
}

/// The parsed location prefix of a compiler message.
type LocationParts<'a> = (Option<&'a str>, Option<u32>, Option<u32>, &'a str);

fn split_location(s: &str) -> Option<LocationParts<'_>> {
    let mut rest = s;
    let mut file: Option<&str> = None;
    let mut line_no = None;
    let mut column = None;

    // The file name may itself contain colons only on Windows; on Linux the
    // first colon separates it from the line number.
    let idx = rest.find(':')?;
    let head = &rest[..idx];
    if let Ok(n) = head.parse::<u32>() {
        // No file name at all: "5:10: error: ..."
        line_no = Some(n);
        rest = &rest[idx + 1..];
        if let Some(idx2) = rest.find(':') {
            if let Ok(c) = rest[..idx2].parse::<u32>() {
                column = Some(c);
                rest = &rest[idx2 + 1..];
            }
        }
    } else {
        file = Some(head);
        rest = &rest[idx + 1..];
        if let Some(idx2) = rest.find(':') {
            if let Ok(n) = rest[..idx2].parse::<u32>() {
                line_no = Some(n);
                rest = &rest[idx2 + 1..];
                if let Some(idx3) = rest.find(':') {
                    if let Ok(c) = rest[..idx3].parse::<u32>() {
                        column = Some(c);
                        rest = &rest[idx3 + 1..];
                    }
                }
            }
        }
    }

    Some((file, line_no, column, rest))
}

fn split_severity(s: &str) -> Option<(Severity, String)> {
    let s = s.trim_start();
    let (sev, msg) = if let Some(rest) = s.strip_prefix("error:") {
        (Severity::Error, rest)
    } else if let Some(rest) = s.strip_prefix("warning:") {
        (Severity::Warning, rest)
    } else if let Some(rest) = s.strip_prefix("note:") {
        (Severity::Note, rest)
    } else if let Some(rest) = s.strip_prefix("info:") {
        (Severity::Info, rest)
    } else {
        // "fatal error:" starts with "error:" only after the word fatal.
        let rest = s.strip_prefix("fatal error:")?;
        (Severity::Error, rest)
    };
    Some((sev, msg.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_gcc_error() {
        let line = "main.cpp:5:10: error: expected ';' before '}' token";
        let d = parse_line(line).expect("diagnostic");
        assert_eq!(d.file, Some(PathBuf::from("main.cpp")));
        assert_eq!(d.line, Some(5));
        assert_eq!(d.column, Some(10));
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.message, "expected ';' before '}' token");
    }

    #[test]
    fn parses_gcc_warning_without_column() {
        let line = "/usr/include/x.h:30: warning: unused variable";
        let d = parse_line(line).expect("diagnostic");
        assert_eq!(d.line, Some(30));
        assert_eq!(d.column, None);
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn parses_note() {
        let d = parse_line("a.c:2:5: note: candidate function not viable").expect("diag");
        assert_eq!(d.severity, Severity::Note);
    }

    #[test]
    fn rejects_non_diagnostic_lines() {
        assert!(parse_line("In file included from main.cpp:").is_none());
        assert!(parse_line("  12 |   int x;").is_none());
        assert!(parse_line("make: *** [Makefile:5: all] Error 1").is_none());
    }

    #[test]
    fn extracts_source_line() {
        let output = "  3 |     int x = ;\nmain.cpp:3:5: error: bad\n";
        let (diags, raw) = parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].source_line.as_deref(), Some("int x = ;"));
        assert!(raw.is_empty());
    }

    #[test]
    fn keeps_raw_output() {
        let output = "g++ -c main.cpp\ng++: fatal error: no input files\n";
        let (diags, raw) = parse(output);
        assert_eq!(diags.len(), 1);
        assert_eq!(raw, vec!["g++ -c main.cpp".to_string()]);
    }

    #[test]
    fn location_formatting() {
        let d = Diagnostic {
            file: Some(PathBuf::from("a.cpp")),
            line: Some(3),
            column: Some(7),
            severity: Severity::Error,
            message: "x".to_string(),
            source_line: None,
        };
        assert_eq!(d.location().unwrap(), "a.cpp:3:7");
    }
}
