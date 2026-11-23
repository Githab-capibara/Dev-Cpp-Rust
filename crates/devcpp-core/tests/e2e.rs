//! End-to-end test: create a project from a template, build it with the
//! Makefile generator, run it and check its output.

use std::path::PathBuf;
use std::process::Command;

use devcpp_core::build::{self, BuildMessage, RunMode};
use devcpp_core::project::Project;
use devcpp_core::templates;

#[test]
fn end_to_end_console_project() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = templates::find_by_name("Console Application").expect("template");

    let dev_path = templates::create(template, "e2e", dir.path()).expect("create project");
    assert!(dev_path.exists(), "the .dev file must exist");
    assert!(dir.path().join("main.cpp").exists());

    let project = Project::load(&dev_path).expect("load project");
    assert_eq!(project.name, "e2e");
    assert_eq!(project.units.len(), 1);

    // Build.
    let job = build::start(&project, RunMode::BuildOnly).expect("start build");
    let mut log = Vec::new();
    let success = loop {
        match job.recv() {
            Ok(BuildMessage::Line(line)) => log.push(line),
            Ok(BuildMessage::Finished(ok)) => break ok,
            Ok(_) => {}
            Err(e) => panic!("build channel closed: {e}"),
        }
    };
    assert!(success, "build failed:\n{}", log.join("\n"));

    let exe = project.executable_path();
    assert!(exe.exists(), "the executable must exist: {}", exe.display());

    // Run.
    let output = Command::new(&exe).output().expect("run the program");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Hello, World!"),
        "unexpected output: {stdout}"
    );
}

#[test]
fn end_to_end_run_mode_streams_program_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = templates::find_by_name("Console Application").expect("template");
    let dev_path = templates::create(template, "runmode", dir.path()).expect("create");
    let project = Project::load(&dev_path).expect("load");

    let job = build::start(&project, RunMode::BuildAndRun).expect("start");
    let mut output = Vec::new();
    let success = loop {
        match job.recv() {
            Ok(BuildMessage::Line(line)) => output.push(line),
            Ok(BuildMessage::Finished(ok)) => break ok,
            Ok(_) => {}
            Err(e) => panic!("channel closed: {e}"),
        }
    };
    assert!(success, "run mode must finish successfully");
    assert!(
        output.iter().any(|line| line.contains("Hello, World!")),
        "program output must be streamed, got: {output:?}"
    );
}

#[test]
fn end_to_end_static_library() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = templates::find_by_name("Static Library").expect("template");
    let dev_path = templates::create(template, "mylib", dir.path()).expect("create");
    let project = Project::load(&dev_path).expect("load");

    let job = build::start(&project, RunMode::BuildOnly).expect("start");
    let success = loop {
        match job.recv() {
            Ok(BuildMessage::Finished(ok)) => break ok,
            Ok(_) => {}
            Err(e) => panic!("channel closed: {e}"),
        }
    };
    assert!(success, "the static library must build");
    let lib: PathBuf = project.executable_path();
    assert!(lib.exists(), "the archive must exist: {}", lib.display());
    assert_eq!(lib.extension().and_then(|e| e.to_str()), Some("a"));
}

#[test]
fn end_to_end_shared_library() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = templates::find_by_name("Dynamic Link Library").expect("template");
    let dev_path = templates::create(template, "myshared", dir.path()).expect("create");
    let project = Project::load(&dev_path).expect("load");

    let job = build::start(&project, RunMode::BuildOnly).expect("start");
    let success = loop {
        match job.recv() {
            Ok(BuildMessage::Finished(ok)) => break ok,
            Ok(_) => {}
            Err(e) => panic!("channel closed: {e}"),
        }
    };
    assert!(success, "the shared library must build");
    let lib = project.executable_path();
    assert!(
        lib.exists(),
        "the shared object must exist: {}",
        lib.display()
    );
}

#[test]
fn generated_makefile_is_valid_gnu_make() {
    let dir = tempfile::tempdir().expect("tempdir");
    let template = templates::find_by_name("Console Application").expect("template");
    let dev_path = templates::create(template, "mk", dir.path()).expect("create");
    let project = Project::load(&dev_path).expect("load");

    let makefile = devcpp_core::makefile::write(&project).expect("write makefile");
    assert!(makefile.exists());

    // `make -n` must parse the file without executing the compiler.
    let output = Command::new("make")
        .arg("-f")
        .arg(&makefile)
        .arg("-n")
        .current_dir(project.directory())
        .output()
        .expect("run make");
    assert!(
        output.status.success(),
        "make failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("g++"),
        "the link line must use g++: {stdout}"
    );
}
