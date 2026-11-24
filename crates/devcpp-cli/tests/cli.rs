//! End-to-end tests of the `devcpp` binary against real projects.

use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

/// The compiled CLI under test (provided by cargo for integration tests).
fn devcpp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_devcpp"))
}

/// `devcpp new` writes the project and prints the `.dev` path.
#[test]
fn new_creates_project_from_template() {
    let dir = tempdir().expect("tempdir");
    let project = dir.path().join("hello");

    let output = devcpp()
        .args(["new", "hello", "--template", "Console Application", "--dir"])
        .arg(&project)
        .output()
        .expect("run devcpp new");
    assert!(
        output.status.success(),
        "devcpp new failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = String::from_utf8_lossy(&output.stdout);
    let dev_path = Path::new(printed.trim());
    assert!(dev_path.exists(), ".dev file must be created");
    assert!(project.join("main.cpp").exists(), "main.cpp must exist");
}

/// An unknown template is rejected before anything is written.
#[test]
fn new_rejects_unknown_template() {
    let dir = tempdir().expect("tempdir");
    let project = dir.path().join("nope");

    let output = devcpp()
        .args(["new", "nope", "--template", "Teleporter", "--dir"])
        .arg(&project)
        .output()
        .expect("run devcpp new");
    assert!(!output.status.success(), "unknown template must fail");
    assert!(
        !project.join("nope.dev").exists(),
        "no project may be created for an unknown template"
    );
}

/// A broken build fails the command with a non-zero exit code.
#[test]
fn build_fails_with_nonzero_exit_on_broken_sources() {
    let dir = tempdir().expect("tempdir");
    let project = dir.path().join("broken");

    let created = devcpp()
        .args([
            "new",
            "broken",
            "--template",
            "Console Application",
            "--dir",
        ])
        .arg(&project)
        .output()
        .expect("run devcpp new");
    assert!(created.status.success());
    let dev_path = String::from_utf8_lossy(&created.stdout).trim().to_string();

    // Break the source on purpose: call an undefined function.
    std::fs::write(
        project.join("main.cpp"),
        "int main() { missing_function(); }\n",
    )
    .expect("write source");

    let output = devcpp()
        .args(["build"])
        .arg(&dev_path)
        .output()
        .expect("run devcpp build");
    assert!(!output.status.success(), "broken build must fail");
    // The CLI streams compiler output to stdout.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| {
            l.contains("error:")
                && l.contains("missing_function")
                && !l.starts_with(char::is_whitespace)
        })
        .collect();
    assert_eq!(
        lines.len(),
        1,
        "each compiler diagnostic must be printed once, got: {lines:?}"
    );
}

/// `devcpp build` on a good project succeeds and writes the binary.
#[test]
fn build_good_project_succeeds() {
    let dir = tempdir().expect("tempdir");
    let project = dir.path().join("good");

    let created = devcpp()
        .args(["new", "good", "--template", "Console Application", "--dir"])
        .arg(&project)
        .output()
        .expect("run devcpp new");
    let dev_path = String::from_utf8_lossy(&created.stdout).trim().to_string();

    let output = devcpp()
        .args(["build"])
        .arg(&dev_path)
        .output()
        .expect("run devcpp build");
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("g++"),
        "the compiler command line must be visible: {stdout}"
    );
}

/// `devcpp run` builds and streams the program output.
#[test]
fn run_streams_program_output() {
    let dir = tempdir().expect("tempdir");
    let project = dir.path().join("runner");

    let created = devcpp()
        .args([
            "new",
            "runner",
            "--template",
            "Console Application",
            "--dir",
        ])
        .arg(&project)
        .output()
        .expect("run devcpp new");
    let dev_path = String::from_utf8_lossy(&created.stdout).trim().to_string();

    let output = devcpp()
        .args(["run"])
        .arg(&dev_path)
        .output()
        .expect("run devcpp run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello, World!"),
        "program output must reach stdout: {stdout}"
    );
}
