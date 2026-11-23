//! Build and run pipeline.
//!
//! Dev-C++ generates a Makefile and shells out to `make`; the same pipeline is
//! implemented here with a cancellable background job and a streaming log, so
//! the GUI stays responsive while the compiler runs.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crossbeam_channel::{unbounded, Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{self, Diagnostic};
use crate::makefile;
use crate::project::Project;

/// What the build pipeline is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildPhase {
    /// Writing the Makefile.
    Preparing,
    /// Running `make`.
    Compiling,
    /// Running the produced program.
    Running,
    /// Finished (successfully or not).
    Finished,
}

/// A message emitted by the build job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildMessage {
    /// Phase transition.
    Phase(BuildPhase),
    /// Raw line of compiler output.
    Line(String),
    /// A parsed diagnostic.
    Diagnostic(Diagnostic),
    /// The job finished; the boolean is "success".
    Finished(bool),
    /// The job failed with this message.
    Error(String),
}

/// Handle to a running build job.
#[derive(Debug)]
pub struct BuildJob {
    receiver: Receiver<BuildMessage>,
    cancel: Arc<AtomicBool>,
    child: Arc<std::sync::Mutex<Option<u32>>>,
}

impl BuildJob {
    /// Returns the next message, blocking until one is available.
    pub fn recv(&self) -> Result<BuildMessage, crossbeam_channel::RecvError> {
        self.receiver.recv()
    }

    /// Returns the next message if one is already queued.
    pub fn try_recv(&self) -> Option<BuildMessage> {
        self.receiver.try_recv().ok()
    }

    /// Requests cancellation; the build process is terminated.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.child.lock() {
            if let Some(pid) = *guard {
                // The child is owned by the build thread; terminate it by PID.
                let _ = Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// What to do after a successful build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Build only.
    BuildOnly,
    /// Build, then run the program.
    BuildAndRun,
    /// Build, then start the program under GDB.
    BuildAndDebug,
}

/// Starts a build job for `project`.
///
/// # Errors
/// Returns an error when the Makefile cannot be generated or the build tool
/// is missing.
pub fn start(project: &Project, mode: RunMode) -> Result<BuildJob, BuildError> {
    let (tx, rx): (Sender<BuildMessage>, Receiver<BuildMessage>) = unbounded();
    let cancel = Arc::new(AtomicBool::new(false));
    let child_slot: Arc<std::sync::Mutex<Option<u32>>> = Arc::new(std::sync::Mutex::new(None));

    let project = project.clone();
    let cancel_clone = Arc::clone(&cancel);
    let cancel_err = Arc::clone(&cancel);
    let child_clone = Arc::clone(&child_slot);
    let child_slot_return = Arc::clone(&child_slot);

    std::thread::spawn(move || {
        let _ = tx.send(BuildMessage::Phase(BuildPhase::Preparing));

        let makefile = match makefile::write(&project) {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(BuildMessage::Error(format!("cannot write Makefile: {e}")));
                let _ = tx.send(BuildMessage::Finished(false));
                return;
            }
        };

        let make_cmd = "make";

        let mut cmd = Command::new(make_cmd);
        cmd.arg("-f")
            .arg(&makefile)
            .arg("-j")
            .arg(num_jobs().to_string())
            .current_dir(project.directory())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(BuildMessage::Error(format!(
                    "cannot start '{make_cmd}': {e}"
                )));
                let _ = tx.send(BuildMessage::Finished(false));
                return;
            }
        };

        if let Ok(mut guard) = child_clone.lock() {
            *guard = Some(child.id());
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tx_stdout = tx.clone();
        let tx_stderr = tx.clone();
        let cancel_out = Arc::clone(&cancel_clone);

        let h_out = std::thread::spawn(move || {
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if cancel_out.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = tx_stdout.send(BuildMessage::Line(line.clone()));
                    if let Some(d) = diagnostics::parse_line(&line) {
                        let _ = tx_stdout.send(BuildMessage::Diagnostic(d));
                    }
                }
            }
        });

        let h_err = std::thread::spawn(move || {
            if let Some(err) = stderr {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if cancel_err.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = tx_stderr.send(BuildMessage::Line(line.clone()));
                    if let Some(d) = diagnostics::parse_line(&line) {
                        let _ = tx_stderr.send(BuildMessage::Diagnostic(d));
                    }
                }
            }
        });

        let status = child.wait();
        let _ = h_out.join();
        let _ = h_err.join();
        if let Ok(mut guard) = child_slot.lock() {
            *guard = None;
        }
        let success = matches!(status, Ok(s) if s.success());
        if !success && !cancel_clone.load(Ordering::SeqCst) {
            let _ = tx.send(BuildMessage::Error("build failed".to_string()));
        }
        if success && mode == RunMode::BuildAndRun {
            let program_ok = run_and_stream(&project, &tx, &cancel_clone);
            if !program_ok {
                // The build itself succeeded, but the program failed.
                let _ = tx.send(BuildMessage::Finished(false));
                return;
            }
        }
        let _ = tx.send(BuildMessage::Finished(success));
    });

    Ok(BuildJob {
        receiver: rx,
        cancel,
        child: child_slot_return,
    })
}

/// Runs the project executable and streams its output as build lines.
///
/// Returns whether the program exited successfully. Used after a successful
/// build in `RunMode::BuildAndRun`, mirroring Dev-C++'s output panel, which
/// shows the program's output under the compiler messages.
fn run_and_stream(project: &Project, tx: &Sender<BuildMessage>, cancel: &Arc<AtomicBool>) -> bool {
    let _ = tx.send(BuildMessage::Phase(BuildPhase::Running));
    let exe = project.executable_path();
    let mut cmd = Command::new(&exe);
    if !project.options.cmd_line_args.is_empty() {
        cmd.args(project.options.cmd_line_args.split_whitespace());
    }
    cmd.current_dir(project.directory())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(BuildMessage::Error(format!(
                "cannot start '{}': {e}",
                exe.display()
            )));
            return false;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let tx_out = tx.clone();
    let tx_err = tx.clone();
    let cancel_out = Arc::clone(cancel);
    let cancel_err = Arc::clone(cancel);

    let h_out = std::thread::spawn(move || {
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if cancel_out.load(Ordering::SeqCst) {
                    break;
                }
                let _ = tx_out.send(BuildMessage::Line(line));
            }
        }
    });
    let h_err = std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if cancel_err.load(Ordering::SeqCst) {
                    break;
                }
                let _ = tx_err.send(BuildMessage::Line(line));
            }
        }
    });

    let status = child.wait();
    let _ = h_out.join();
    let _ = h_err.join();
    let ok = matches!(status, Ok(s) if s.success());
    if !ok && !cancel.load(Ordering::SeqCst) {
        let _ = tx.send(BuildMessage::Error(
            "the program exited with an error".to_string(),
        ));
    }
    let _ = tx.send(BuildMessage::Phase(BuildPhase::Finished));
    ok
}

/// Runs the project's executable directly (without rebuilding).
///
/// # Errors
/// Returns an error when the executable does not exist or cannot be started.
pub fn run(project: &Project) -> Result<std::process::Child, BuildError> {
    let exe = project.executable_path();
    if !exe.exists() {
        return Err(BuildError::ExecutableMissing(exe));
    }
    let mut cmd = Command::new(&exe);
    if !project.options.cmd_line_args.is_empty() {
        cmd.args(project.options.cmd_line_args.split_whitespace());
    }
    cmd.current_dir(project.directory());
    cmd.spawn().map_err(BuildError::Io)
}

/// Errors of the build pipeline.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The compiled program does not exist.
    #[error("executable not found: {0}")]
    ExecutableMissing(PathBuf),
    /// I/O or process error.
    #[error("build error: {0}")]
    Io(#[from] std::io::Error),
}

fn num_jobs() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// Returns the directory of the project's executable.
#[must_use]
pub fn binary_dir(project: &Project) -> PathBuf {
    project
        .executable_path()
        .parent()
        .map_or_else(|| project.directory(), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Unit};

    #[test]
    fn builds_and_runs_hello_world() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("main.cpp");
        std::fs::write(
            &src,
            "#include <cstdio>\nint main(){std::puts(\"hello from devcpp\");return 0;}\n",
        )
        .expect("write source");

        let mut project = Project::new(dir.path().join("hello.dev"));
        project.units.push(Unit {
            file_name: "main.cpp".to_string(),
            compile_cpp: Some(true),
            ..Unit::default()
        });

        let job = start(&project, RunMode::BuildOnly).expect("start build");
        let mut success = false;
        let mut lines = Vec::new();
        loop {
            match job.recv() {
                Ok(BuildMessage::Finished(ok)) => {
                    success = ok;
                    break;
                }
                Ok(BuildMessage::Line(l)) => lines.push(l),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(success, "build failed, log: {lines:?}");
        assert!(project.executable_path().exists());

        let mut child = run(&project).expect("run");
        let status = child.wait().expect("wait");
        assert!(status.success());
    }

    #[test]
    fn runs_program_after_build_in_build_and_run_mode() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("main.cpp");
        std::fs::write(
            &src,
            "#include <cstdio>\nint main(){std::puts(\"hello from run mode\");return 0;}\n",
        )
        .expect("write source");

        let mut project = Project::new(dir.path().join("hello.dev"));
        project.units.push(Unit {
            file_name: "main.cpp".to_string(),
            compile_cpp: Some(true),
            ..Unit::default()
        });

        let job = start(&project, RunMode::BuildAndRun).expect("start build");
        let mut output: Vec<String> = Vec::new();
        let mut finished = None;
        loop {
            match job.recv() {
                Ok(BuildMessage::Line(line)) => output.push(line),
                Ok(BuildMessage::Finished(ok)) => {
                    finished = Some(ok);
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert_eq!(finished, Some(true), "output: {output:?}");
        assert!(
            output.iter().any(|l| l.contains("hello from run mode")),
            "program output missing: {output:?}"
        );
    }

    #[test]
    fn reports_compile_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bad.c"), "int main(){ return zzz; }\n").expect("write");

        let mut project = Project::new(dir.path().join("bad.dev"));
        project.units.push(Unit {
            file_name: "bad.c".to_string(),
            compile_cpp: Some(false),
            ..Unit::default()
        });

        let job = start(&project, RunMode::BuildOnly).expect("start");
        let mut saw_diag = false;
        let mut success = true;
        loop {
            match job.recv() {
                Ok(BuildMessage::Diagnostic(d)) => {
                    if d.severity == crate::diagnostics::Severity::Error {
                        saw_diag = true;
                    }
                }
                Ok(BuildMessage::Finished(ok)) => {
                    success = ok;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(!success);
        assert!(saw_diag, "expected an error diagnostic");
    }
}
