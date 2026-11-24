//! `devcpp` — command line front-end of the IDE.
//!
//! Crate-level documentation for the `devcpp` binary.
//!
//! Useful for scripting builds and for CI, and it exercises the same core the
//! GTK front-end uses.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use devcpp_core::build::{self, BuildMessage, RunMode};
use devcpp_core::project::Project;
use devcpp_core::toolchain::Toolchain;

#[derive(Debug, Parser)]
#[command(
    name = "devcpp",
    version,
    about = "Native Linux C/C++ IDE — Rust clone of Embarcadero Dev-C++"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build the project.
    Build {
        /// Project file (.dev).
        project: PathBuf,
        /// Rebuild everything from scratch.
        #[arg(long)]
        clean: bool,
    },
    /// Build and run the project.
    Run {
        /// Project file (.dev).
        project: PathBuf,
    },
    /// Build and start the project under GDB.
    Debug {
        /// Project file (.dev).
        project: PathBuf,
    },
    /// Print information about the project.
    Info {
        /// Project file (.dev).
        project: PathBuf,
    },
    /// Create a new project from a template.
    New {
        /// Project name (becomes `name.dev`).
        name: String,
        /// Template name (see `devcpp templates`).
        #[arg(long, default_value = "Console Application")]
        template: String,
        /// Target directory for the project.
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// List the tools available on this machine.
    Tools,
    /// List the project templates.
    Templates,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("devcpp: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Build { project, clean } => {
            let project = load(&project)?;
            if clean {
                std::process::Command::new("make")
                    .arg("-f")
                    .arg(project.makefile_name())
                    .arg("clean")
                    .current_dir(project.directory())
                    .status()
                    .ok();
            }
            build_and_wait(&project, RunMode::BuildOnly)
        }
        Command::Run { project } => {
            let project = load(&project)?;
            let code = build_and_wait(&project, RunMode::BuildOnly)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            let mut child = build::run(&project).context("cannot run the program")?;
            let status = child.wait().context("waiting for the program")?;
            Ok(if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Debug { project } => {
            let project = load(&project)?;
            let code = build_and_wait(&project, RunMode::BuildOnly)?;
            if code != ExitCode::SUCCESS {
                return Ok(code);
            }
            let exe = project.executable_path();
            let mut child = std::process::Command::new("gdb")
                .arg("--args")
                .arg(&exe)
                .spawn()
                .context("cannot start GDB")?;
            let status = child.wait().context("waiting for GDB")?;
            Ok(if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Info { project } => {
            let project = load(&project)?;
            println!("Name:       {}", project.name);
            println!("File:       {}", project.file_name.display());
            println!("Type:       {:?}", project.options.project_type);
            println!("Executable: {}", project.executable_path().display());
            println!("Units:      {}", project.units.len());
            for unit in &project.units {
                println!("  - {}", unit.file_name);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::New {
            name,
            template,
            dir,
        } => {
            let Some(template) = devcpp_core::templates::find_by_name(&template) else {
                anyhow::bail!("unknown template: {template} (see: devcpp templates)")
            };
            let dev_path = devcpp_core::templates::create(template, &name, &dir)?;
            println!("{}", dev_path.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Tools => {
            let tc = Toolchain::detect();
            println!("C compiler:     {:?}", tc.c_compiler.path);
            println!("  version:      {:?}", tc.c_compiler.version);
            println!("C++ compiler:   {:?}", tc.cpp_compiler.path);
            println!("Debugger:       {:?}", tc.debugger.path);
            println!("Make:           {:?}", tc.make.path);
            println!("Formatter:      {:?}", tc.astyle.path);
            println!("Profiler:       {:?}", tc.gprof.path);
            Ok(ExitCode::SUCCESS)
        }
        Command::Templates => {
            for template in devcpp_core::templates::TEMPLATES {
                println!("{} — {}", template.name, template.description);
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load(path: &std::path::Path) -> Result<Project> {
    Project::load(path).map_err(|e| anyhow::anyhow!("{e}"))
}

fn build_and_wait(project: &Project, mode: RunMode) -> Result<ExitCode> {
    let job = build::start(project, mode).context("cannot start the build")?;
    let success;
    loop {
        match job.recv() {
            // Diagnostics are not printed separately: the raw compiler line
            // already carries file, line and message, and printing both
            // duplicated every error in the log.
            Ok(BuildMessage::Line(line)) => println!("{line}"),
            Ok(BuildMessage::Diagnostic(_)) => {}
            Ok(BuildMessage::Error(e)) => eprintln!("devcpp: {e}"),
            Ok(BuildMessage::Finished(ok)) => {
                success = ok;
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("devcpp: build channel closed: {e}");
                success = false;
                break;
            }
        }
    }
    Ok(if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
