//! GDB/MI debugger client.
//!
//! Dev-C++ embeds GDB and talks the GDB/Machine-Interface protocol
//! (`Source/Debugger.pas`, `Source/DebugReader.pas`). This module implements
//! the same protocol subset on Linux:
//!
//! * breakpoint set / delete / list
//! * run, continue, step, next, finish
//! * stack frames and frame selection
//! * local variables and watches
//! * program output
//!
//! Commands are written to GDB's stdin; a reader thread parses the answers
//! (see [`crate::mi`]) and turns them into [`DebugEvent`]s, so the debuggee's
//! console output never blocks the UI.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::mi::{self, Value};

/// A source position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Source file.
    pub file: PathBuf,
    /// 1-based line.
    pub line: u32,
}

/// A breakpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    /// GDB breakpoint number.
    pub number: u32,
    /// Source location.
    pub location: Location,
    /// Whether the breakpoint is enabled.
    pub enabled: bool,
    /// Number of hits, when GDB reports them.
    pub hits: Option<u32>,
    /// Conditional expression, if any.
    pub condition: Option<String>,
}

/// One stack frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackFrame {
    /// Frame index, 0 = innermost.
    pub level: u32,
    /// Function name, when known.
    pub function: Option<String>,
    /// Source location, when known.
    pub location: Option<Location>,
    /// Address of the frame.
    pub address: Option<String>,
}

/// A variable visible in the current scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    /// Variable name.
    pub name: String,
    /// Current value, as printed by GDB.
    pub value: String,
    /// Type name, when known.
    pub type_name: Option<String>,
}

/// Execution state of the debuggee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugState {
    /// GDB is idle, the program has not been started.
    Idle,
    /// The program is running.
    Running,
    /// The program is stopped at a breakpoint or a step.
    Stopped,
    /// The program exited.
    Exited,
}

/// Events delivered by the debugger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugEvent {
    /// GDB printed a line on its console.
    Console(String),
    /// The debuggee printed a line.
    ProgramOutput(String),
    /// Execution state changed.
    State(DebugState),
    /// The program exited with the given code.
    Exited(i32),
    /// The current stack.
    Stack(Vec<StackFrame>),
    /// The current frame's variables.
    Variables(Vec<Variable>),
    /// The breakpoint list changed.
    Breakpoints(Vec<Breakpoint>),
    /// A protocol error occurred.
    Error(String),
}

/// A GDB/MI session.
#[allow(dead_code)]
pub struct Debugger {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    tx: crossbeam_channel::Sender<DebugEvent>,
    rx: crossbeam_channel::Receiver<DebugEvent>,
    running: Arc<AtomicBool>,
    breakpoints: Arc<Mutex<Vec<Breakpoint>>>,
    stack: Arc<Mutex<Vec<StackFrame>>>,
    variables: Arc<Mutex<Vec<Variable>>>,
    state: Arc<Mutex<DebugState>>,
    command_counter: AtomicU32,
    frame: Arc<Mutex<u32>>,
}

impl std::fmt::Debug for Debugger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Debugger")
            .field("state", &*self.state.lock().unwrap())
            .finish_non_exhaustive()
    }
}

impl Debugger {
    /// Starts `gdb --interpreter=mi2` and returns a session.
    ///
    /// # Errors
    /// Returns an error when GDB cannot be started.
    pub fn start(gdb: &str) -> Result<Self, DebugError> {
        let mut child = Command::new(gdb)
            .arg("--interpreter=mi2")
            .arg("--quiet")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| DebugError::Spawn(source.to_string()))?;

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let (tx, rx): (
            crossbeam_channel::Sender<DebugEvent>,
            crossbeam_channel::Receiver<DebugEvent>,
        ) = crossbeam_channel::unbounded();

        let running = Arc::new(AtomicBool::new(false));
        let breakpoints = Arc::new(Mutex::new(Vec::new()));
        let stack = Arc::new(Mutex::new(Vec::new()));
        let variables = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(DebugState::Idle));

        if let Some(stdout) = stdout {
            let reader_running = Arc::clone(&running);
            let reader_bp = Arc::clone(&breakpoints);
            let reader_stack = Arc::clone(&stack);
            let reader_vars = Arc::clone(&variables);
            let reader_state = Arc::clone(&state);
            let reader_tx = tx.clone();
            std::thread::spawn(move || {
                read_mi_stream(
                    stdout,
                    reader_tx,
                    reader_running,
                    reader_bp,
                    reader_stack,
                    reader_vars,
                    reader_state,
                );
            });
        }

        Ok(Self {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            tx,
            rx,
            running,
            breakpoints,
            stack,
            variables,
            state,
            command_counter: AtomicU32::new(0),
            frame: Arc::new(Mutex::new(0)),
        })
    }

    /// Sends a raw MI command.
    ///
    /// # Errors
    /// Returns an error when GDB is not running.
    pub fn send(&self, command: &str) -> Result<(), DebugError> {
        let mut guard = self.stdin.lock().unwrap();
        let stdin = guard.as_mut().ok_or(DebugError::Closed)?;
        writeln!(stdin, "{command}").map_err(|e| DebugError::Io(e.to_string()))?;
        stdin.flush().map_err(|e| DebugError::Io(e.to_string()))?;
        Ok(())
    }

    /// Waits for the next event.
    ///
    /// # Errors
    /// Returns an error when the channel is closed.
    pub fn recv(&self) -> Result<DebugEvent, DebugError> {
        self.rx.recv().map_err(|e| DebugError::Io(e.to_string()))
    }

    /// Returns the next event if one is queued.
    #[must_use]
    pub fn try_recv(&self) -> Option<DebugEvent> {
        self.rx.try_recv().ok()
    }

    /// Waits for the next event, but no longer than `timeout`.
    ///
    /// # Errors
    /// Returns an error when the channel is closed; `Ok(None)` means the
    /// timeout expired.
    pub fn recv_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<DebugEvent>, DebugError> {
        match self.rx.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(None),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                Err(DebugError::Io("debugger channel closed".to_string()))
            }
        }
    }

    /// Current execution state.
    #[must_use]
    pub fn state(&self) -> DebugState {
        *self.state.lock().unwrap()
    }

    /// Known breakpoints.
    #[must_use]
    pub fn breakpoints(&self) -> Vec<Breakpoint> {
        self.breakpoints.lock().unwrap().clone()
    }

    /// Last known stack.
    #[must_use]
    pub fn stack(&self) -> Vec<StackFrame> {
        self.stack.lock().unwrap().clone()
    }

    /// Last known variables.
    #[must_use]
    pub fn variables(&self) -> Vec<Variable> {
        self.variables.lock().unwrap().clone()
    }

    /// Loads the program to be debugged.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn load_program(&self, program: &Path) -> Result<(), DebugError> {
        let quoted = gdb_quote(&program.to_string_lossy());
        self.send(&format!("-file-exec-and-symbols {quoted}"))?;
        Ok(())
    }

    /// Sets a source breakpoint.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn set_breakpoint(&self, file: &Path, line: u32) -> Result<u32, DebugError> {
        // GDB/MI expects `-break-insert "file:line"`.
        let quoted = gdb_quote(&format!("{}:{}", file.to_string_lossy(), line));
        self.send(&format!("-break-insert {quoted}"))?;
        // GDB confirms asynchronously with `=breakpoint-created`; until that
        // arrives we report a provisional number.
        let mut bps = self.breakpoints.lock().unwrap();
        let number = bps.iter().map(|b| b.number).max().unwrap_or(0) + 1;
        bps.push(Breakpoint {
            number,
            location: Location {
                file: file.to_path_buf(),
                line,
            },
            enabled: true,
            hits: None,
            condition: None,
        });
        Ok(number)
    }

    /// Deletes a breakpoint.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn delete_breakpoint(&self, number: u32) -> Result<(), DebugError> {
        self.send(&format!("-break-delete {number}"))?;
        self.breakpoints
            .lock()
            .unwrap()
            .retain(|b| b.number != number);
        Ok(())
    }

    /// Starts the program.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn run(&self) -> Result<(), DebugError> {
        self.set_state(DebugState::Running);
        self.send("-exec-run")?;
        Ok(())
    }

    /// Continues execution.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn cont(&self) -> Result<(), DebugError> {
        self.set_state(DebugState::Running);
        self.send("-exec-continue")?;
        Ok(())
    }

    /// Steps into the next source line.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn step(&self) -> Result<(), DebugError> {
        self.set_state(DebugState::Running);
        self.send("-exec-step")?;
        Ok(())
    }

    /// Steps over the next source line.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn next(&self) -> Result<(), DebugError> {
        self.set_state(DebugState::Running);
        self.send("-exec-next")?;
        Ok(())
    }

    /// Runs until the current function returns.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn finish(&self) -> Result<(), DebugError> {
        self.set_state(DebugState::Running);
        self.send("-exec-finish")?;
        Ok(())
    }

    /// Interrupts the running program.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn interrupt(&self) -> Result<(), DebugError> {
        self.send("-exec-interrupt")?;
        Ok(())
    }

    /// Terminates the debuggee and quits GDB.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn kill(&self) -> Result<(), DebugError> {
        let _ = self.send("-exec-abort");
        let _ = self.send("-gdb-exit");
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
        *self.stdin.lock().unwrap() = None;
        self.set_state(DebugState::Idle);
        Ok(())
    }

    /// Requests the current stack.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn refresh_stack(&self) -> Result<(), DebugError> {
        self.send("-stack-list-frames")?;
        Ok(())
    }

    /// Requests the variables of the current frame.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn refresh_variables(&self) -> Result<(), DebugError> {
        let level = *self.frame.lock().unwrap();
        self.send(&format!("-stack-list-variables --frame {level}"))?;
        Ok(())
    }

    /// Selects a stack frame.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn select_frame(&self, level: u32) -> Result<(), DebugError> {
        *self.frame.lock().unwrap() = level;
        self.send(&format!("-stack-select-frame {level}"))?;
        Ok(())
    }

    /// Evaluates an expression.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn evaluate(&self, expression: &str) -> Result<(), DebugError> {
        self.send(&format!("-data-evaluate-expression {expression}"))?;
        Ok(())
    }

    /// Sends a command that produces output; the answer is appended to `out`.
    ///
    /// # Errors
    /// Returns an error when GDB rejects the command.
    pub fn evaluate_into(&self, expression: &str, out: &mut String) -> Result<(), DebugError> {
        self.send(&format!("-data-evaluate-expression {expression}"))?;
        out.push_str(expression);
        Ok(())
    }

    fn set_state(&self, state: DebugState) {
        *self.state.lock().unwrap() = state;
    }
}

impl Drop for Debugger {
    fn drop(&mut self) {
        let _ = self.kill();
    }
}

type BpList = Arc<Mutex<Vec<Breakpoint>>>;
type FrameList = Arc<Mutex<Vec<StackFrame>>>;
type VarList = Arc<Mutex<Vec<Variable>>>;
type StateRef = Arc<Mutex<DebugState>>;

#[allow(clippy::too_many_arguments)]
fn read_mi_stream(
    stdout: ChildStdout,
    tx: crossbeam_channel::Sender<DebugEvent>,
    running: Arc<AtomicBool>,
    breakpoints: BpList,
    stack: FrameList,
    variables: VarList,
    state: StateRef,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix('~') {
            let _ = tx.send(DebugEvent::Console(unescape_mi(rest)));
            continue;
        }
        if let Some(rest) = line.strip_prefix('@') {
            let _ = tx.send(DebugEvent::ProgramOutput(unescape_mi(rest)));
            continue;
        }
        let Some(record) = mi::parse(&line) else {
            // Non-MI output (GDB's own messages).
            if !line.trim().is_empty() {
                let _ = tx.send(DebugEvent::Console(line));
            }
            continue;
        };
        handle_record(&record, &tx, &breakpoints, &stack, &variables, &state);
    }
    running.store(false, Ordering::SeqCst);
}

fn handle_record(
    record: &mi::Record,
    tx: &crossbeam_channel::Sender<DebugEvent>,
    breakpoints: &BpList,
    stack: &FrameList,
    variables: &VarList,
    state: &StateRef,
) {
    match record.class.as_str() {
        "breakpoint-created" | "breakpoint-modified" => {
            if let Some(bkpt) = record.get("bkpt") {
                if let Some(bp) = parse_breakpoint(bkpt) {
                    let mut list = breakpoints.lock().unwrap();
                    list.retain(|b| b.number != bp.number);
                    list.push(bp);
                    list.sort_by_key(|b| b.number);
                    let snapshot = list.clone();
                    drop(list);
                    let _ = tx.send(DebugEvent::Breakpoints(snapshot));
                }
            }
        }
        "breakpoint-deleted" => {
            if let Some(id) = record.get("id").and_then(Value::as_str) {
                if let Ok(number) = id.parse::<u32>() {
                    breakpoints.lock().unwrap().retain(|b| b.number != number);
                    let snapshot = breakpoints.lock().unwrap().clone();
                    let _ = tx.send(DebugEvent::Breakpoints(snapshot));
                }
            }
        }
        "stopped" => {
            *state.lock().unwrap() = DebugState::Stopped;
            let _ = tx.send(DebugEvent::State(DebugState::Stopped));
            if let Some(reason) = record.get_str("reason") {
                if reason.contains("exited") {
                    let code = record
                        .path(&["exit-code"])
                        .and_then(Value::as_str)
                        .and_then(|c| c.parse::<i32>().ok())
                        .unwrap_or(0);
                    let _ = tx.send(DebugEvent::Exited(code));
                }
            }
            if let Some(frame) = record.get("frame") {
                if let Some(f) = parse_frame(frame, 0) {
                    *stack.lock().unwrap() = vec![f];
                    let snapshot = stack.lock().unwrap().clone();
                    let _ = tx.send(DebugEvent::Stack(snapshot));
                }
            }
        }
        "running" => {
            *state.lock().unwrap() = DebugState::Running;
            let _ = tx.send(DebugEvent::State(DebugState::Running));
        }
        "exited" => {
            *state.lock().unwrap() = DebugState::Exited;
            let code = record
                .get("exit-code")
                .and_then(Value::as_str)
                .and_then(|c| c.parse::<i32>().ok())
                .unwrap_or(0);
            let _ = tx.send(DebugEvent::Exited(code));
        }
        "done" => {
            // `-break-insert` answers with `^done,bkpt={...}`.
            if let Some(bkpt) = record.get("bkpt") {
                if let Some(bp) = parse_breakpoint(bkpt) {
                    let mut list = breakpoints.lock().unwrap();
                    list.retain(|b| b.number != bp.number);
                    list.push(bp);
                    list.sort_by_key(|b| b.number);
                    let snapshot = list.clone();
                    drop(list);
                    let _ = tx.send(DebugEvent::Breakpoints(snapshot));
                }
            }
            if let Some(list) = record.get("stack").and_then(Value::as_list) {
                let frames: Vec<StackFrame> = list
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| parse_frame(v, i as u32))
                    .collect();
                if !frames.is_empty() {
                    *stack.lock().unwrap() = frames.clone();
                    let _ = tx.send(DebugEvent::Stack(frames));
                }
            }
            if let Some(list) = record.get("variables").and_then(Value::as_list) {
                let vars: Vec<Variable> = list.iter().filter_map(parse_variable).collect();
                *variables.lock().unwrap() = vars.clone();
                let _ = tx.send(DebugEvent::Variables(vars));
            }
            if record.get("stack").is_some() || record.get("variables").is_some() {
                *state.lock().unwrap() = DebugState::Stopped;
                let _ = tx.send(DebugEvent::State(DebugState::Stopped));
            }
        }
        "error" => {
            let msg = record.get_str("msg").unwrap_or("GDB error").to_string();
            let _ = tx.send(DebugEvent::Error(msg));
        }
        _ => {}
    }
}

fn parse_breakpoint(value: &Value) -> Option<Breakpoint> {
    let number = value.get_str("number")?.parse::<u32>().ok()?;
    let file = value.get_str("file").map_or_else(
        || PathBuf::from(value.get_str("fullname").unwrap_or_default()),
        PathBuf::from,
    );
    let line = value
        .get_str("line")
        .and_then(|l| l.parse::<u32>().ok())
        .unwrap_or(0);
    Some(Breakpoint {
        number,
        location: Location { file, line },
        enabled: value
            .get_str("enabled")
            .is_none_or(|e| e == "y" || e == "1"),
        hits: value.get_str("times").and_then(|h| h.parse::<u32>().ok()),
        condition: value.get_str("cond").map(ToString::to_string),
    })
}

fn parse_frame(value: &Value, level: u32) -> Option<StackFrame> {
    // GDB encodes stack frames as `frame={level="0",func="main",...}`.
    let inner = value.get("frame").unwrap_or(value);
    let level = inner
        .get_str("level")
        .and_then(|l| l.parse::<u32>().ok())
        .unwrap_or(level);
    let function = inner.get_str("func").map(ToString::to_string);
    let address = inner.get_str("addr").map(ToString::to_string);
    let location = match (inner.get_str("file"), inner.get_str("line")) {
        (Some(file), Some(line)) => Some(Location {
            file: PathBuf::from(file),
            line: line.parse::<u32>().unwrap_or(0),
        }),
        _ => None,
    };
    if function.is_none() && location.is_none() {
        return None;
    }
    Some(StackFrame {
        level,
        function,
        location,
        address,
    })
}

fn parse_variable(value: &Value) -> Option<Variable> {
    let name = value.get_str("name")?.to_string();
    let val = value.get_str("value").unwrap_or_default().to_string();
    let type_name = value.get_str("type").map(ToString::to_string);
    Some(Variable {
        name,
        value: val,
        type_name,
    })
}

/// Escapes a string for a GDB MI command.
#[must_use]
pub fn gdb_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// Unescapes C-style escapes used by GDB MI output.
#[must_use]
pub fn unescape_mi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('0') => out.push('\0'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Errors of the debugger client.
#[derive(Debug, thiserror::Error)]
pub enum DebugError {
    /// GDB could not be started.
    #[error("cannot start GDB: {0}")]
    Spawn(String),
    /// I/O error while talking to GDB.
    #[error("debugger I/O error: {0}")]
    Io(String),
    /// The GDB session is closed.
    #[error("debugger session is closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn quotes_paths() {
        assert_eq!(gdb_quote("/tmp/a b.cpp"), "\"/tmp/a b.cpp\"");
        assert_eq!(gdb_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn unescapes_mi_strings() {
        assert_eq!(unescape_mi("a\\nb"), "a\nb");
        assert_eq!(unescape_mi("tab\\there"), "tab\there");
        assert_eq!(unescape_mi("q\\\"q"), "q\"q");
    }

    #[test]
    fn starts_gdb_and_answers_ping() {
        let dbg = Debugger::start("gdb").expect("start gdb");
        dbg.send("-gdb-version").expect("send");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut saw_console = false;
        while std::time::Instant::now() < deadline {
            match dbg.recv() {
                Ok(DebugEvent::Console(_)) => {
                    saw_console = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_console, "expected a console answer from GDB");
        dbg.kill().expect("kill");
    }

    #[test]
    fn stops_at_breakpoint_and_reports_stack() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("main.c");
        std::fs::write(&src, "int main(){int a=1; a=2; return a;}\n").expect("write");
        let exe = dir.path().join("a.out");
        let status = Command::new("gcc")
            .arg("-g")
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .status()
            .expect("compile");
        assert!(status.success());

        let dbg = Debugger::start("gdb").expect("start gdb");
        dbg.load_program(&exe).expect("load");
        dbg.set_breakpoint(&src, 1).expect("breakpoint");
        dbg.run().expect("run");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut stopped = false;
        while std::time::Instant::now() < deadline {
            match dbg.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(Some(DebugEvent::State(DebugState::Stopped))) => {
                    stopped = true;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) => {}
                Err(_) => break,
            }
        }
        assert!(stopped, "expected the program to stop at the breakpoint");
        dbg.kill().expect("kill");
    }

    #[test]
    fn reports_breakpoint_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("main.c");
        std::fs::write(&src, "int main(){return 0;}\n").expect("write");
        let exe = dir.path().join("a.out");
        assert!(Command::new("gcc")
            .arg("-g")
            .arg(&src)
            .arg("-o")
            .arg(&exe)
            .status()
            .expect("compile")
            .success());

        let dbg = Debugger::start("gdb").expect("start gdb");
        dbg.load_program(&exe).expect("load");
        dbg.set_breakpoint(&src, 1).expect("breakpoint");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut confirmed = false;
        while std::time::Instant::now() < deadline {
            match dbg.recv_timeout(std::time::Duration::from_millis(500)) {
                Ok(Some(DebugEvent::Breakpoints(list))) => {
                    if list.iter().any(|b| b.location.line == 1) {
                        confirmed = true;
                        break;
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) => {}
                Err(_) => break,
            }
        }
        assert!(confirmed, "expected a breakpoint-created record");
        dbg.kill().expect("kill");
    }
}
