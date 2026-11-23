//! Makefile generation, byte-compatible with Dev-C++'s `TCompiler`.
//!
//! Reference: `Source/Compiler.pas` (`NewMakeFile`, `WriteMakeHeader`,
//! `WriteMakeDefines`, `WriteMakeTarget`, `WriteMakeIncludes`,
//! `WriteMakeObjFilesRules`, `WriteMakeClean`, `CreateStandardMakefile`).
//!
//! The generated Makefile is plain GNU make and works with the system GCC on
//! Linux. Windows-only concepts (`windres`, `.rc` resources) are dropped.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::project::{Project, ProjectType};

/// Compiler switches the original Dev-C++ exposes as checkboxes. Stored as
/// index => enabled flag, exactly like `Project/CompilerSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerSwitch {
    /// Index in the settings vector (stable across versions).
    pub index: usize,
    /// GNU make / GCC switch.
    pub flag: &'static str,
    /// Human readable label shown in the "Project Options" dialog.
    pub label: &'static str,
    /// Applies to C compilation.
    pub is_c: bool,
    /// Applies to C++ compilation.
    pub is_cpp: bool,
}

/// The default switch table, modelled on Dev-C++'s compiler options page.
pub const DEFAULT_SWITCHES: &[CompilerSwitch] = &[
    CompilerSwitch {
        index: 0,
        flag: "-Wall",
        label: "Wall",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 1,
        flag: "-Wextra",
        label: "Extra",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 2,
        flag: "-O2",
        label: "Optimize Level 2",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 3,
        flag: "-g",
        label: "Generate Debug Info",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 4,
        flag: "-std=c++17",
        label: "C++17",
        is_c: false,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 5,
        flag: "-std=c++20",
        label: "C++20",
        is_c: false,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 6,
        flag: "-fPIC",
        label: "Position Independent Code",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 7,
        flag: "-pedantic",
        label: "Pedantic",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 8,
        flag: "-Wshadow",
        label: "Shadow",
        is_c: true,
        is_cpp: true,
    },
    CompilerSwitch {
        index: 9,
        flag: "-Wconversion",
        label: "Conversion",
        is_c: true,
        is_cpp: true,
    },
];

/// Builds the `CFLAGS`/`CXXFLAGS` strings from the project's option vector.
#[must_use]
pub fn compile_flags(project: &Project, cpp: bool) -> String {
    let mut flags = String::new();
    for sw in DEFAULT_SWITCHES {
        let applies = if cpp { sw.is_cpp } else { sw.is_c };
        if !applies {
            continue;
        }
        let enabled = project
            .options
            .compiler_options
            .get(sw.index)
            .is_some_and(|&v| v != 0);
        if enabled {
            let _ = write!(flags, " {}", sw.flag);
        }
    }
    flags
}

/// Result of generating a Makefile.
#[derive(Debug, Clone)]
pub struct GeneratedMakefile {
    /// Path the Makefile was (or would be) written to.
    pub path: PathBuf,
    /// The Makefile content.
    pub content: String,
}

/// Generates the Makefile for `project`.
///
/// # Errors
/// Returns an error when the project has no compilable units.
pub fn generate(project: &Project) -> Result<GeneratedMakefile, MakefileError> {
    let sources: Vec<_> = project
        .units
        .iter()
        .filter(|u| u.compile && is_source(&u.file_name))
        .collect();

    if sources.is_empty() {
        return Err(MakefileError::NoSources);
    }

    let dir = project.directory();
    let makefile_path = project.makefile_name();

    let mut objects = Vec::new();
    let mut link_objects = Vec::new();
    for unit in &sources {
        let abs = unit.absolute_path(&dir);
        let rel = paths::relative_path(&makefile_path, &abs);
        let obj = object_path(project, &dir, &rel);
        let obj_rel = paths::relative_path(&makefile_path, &obj);
        objects.push(make_path(&obj_rel));
        if unit.link {
            link_objects.push(make_path(&obj_rel));
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "# Project: {}", project.name);
    let _ = writeln!(
        out,
        "# Makefile created by devcpp (Rust clone of Embarcadero Dev-C++)"
    );
    out.push('\n');

    let is_cpp_project = project.options.use_gpp
        || sources
            .iter()
            .any(|u| u.compile_cpp.unwrap_or(project.options.use_gpp));
    let (cc_var, cxx_var) = if is_cpp_project {
        ("CXX", "CXX")
    } else {
        ("CC", "CC")
    };
    let linker = if project.options.linker_cmd.is_empty() {
        if is_cpp_project {
            "g++"
        } else {
            "gcc"
        }
    } else {
        project.options.linker_cmd.as_str()
    };

    let _ = writeln!(out, "{} = {}", cc_var, project.options.compiler_cmd);
    let _ = writeln!(out, "{} = {}", cxx_var, project.options.cpp_compiler_cmd);
    let _ = writeln!(out, "WINDRES = windres");
    let _ = writeln!(out, "RM = rm -f");
    let _ = writeln!(out, "CFLAGS ={}", compile_flags(project, false));
    let _ = writeln!(out, "CXXFLAGS ={}", compile_flags(project, true));

    let include_dirs: Vec<String> = project
        .options
        .includes
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| make_path(&paths::expand_file(&dir, s)))
        .collect();
    for inc in &include_dirs {
        let _ = writeln!(out, "INCLUDES += -I{inc}");
    }

    let libs: Vec<String> = project
        .options
        .libs
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if !libs.is_empty() {
        let _ = writeln!(out, "LIBS = {}", libs.join(" "));
    } else {
        out.push_str("LIBS =\n");
    }

    let extra_objs: Vec<String> = project
        .options
        .obj_files
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| make_path(&paths::expand_file(&dir, s)))
        .collect();
    if !extra_objs.is_empty() {
        let _ = writeln!(out, "OBJECTS = {}", extra_objs.join(" "));
    } else {
        out.push_str("OBJECTS =\n");
    }

    let bin_rel = paths::relative_path(&makefile_path, &project.executable_path());
    let _ = writeln!(out, "BIN = {}", make_path(&bin_rel));
    let _ = writeln!(out, "OBJ = {}", objects.join(" "));
    let _ = writeln!(out, "LINKOBJ = {}", link_objects.join(" "));
    out.push('\n');

    out.push_str(".PHONY: all clean\n\n");
    out.push_str("all: $(BIN)\n\n");

    match project.options.project_type {
        ProjectType::StaticLibrary => {
            let _ = writeln!(out, "$(BIN): $(OBJ)\n\t$(RM) $@\n\tar rcs $@ $(OBJ)");
        }
        ProjectType::DynamicLibrary => {
            let _ = writeln!(
                out,
                "$(BIN): $(OBJ)\n\t$(RM) $@\n\t{linker} -shared $(LINKOBJ) -o $@ $(LIBS)"
            );
        }
        _ => {
            let _ = writeln!(
                out,
                "$(BIN): $(OBJ)\n\t{linker} $(LINKOBJ) $(OBJECTS) -o $@ $(LIBS)"
            );
        }
    }
    out.push('\n');

    for unit in &sources {
        let abs = unit.absolute_path(&dir);
        let rel = paths::relative_path(&makefile_path, &abs);
        let obj = object_path(project, &dir, &rel);
        let obj_rel = paths::relative_path(&makefile_path, &obj);
        let compile_as_cpp = unit.compile_cpp.unwrap_or(project.options.use_gpp);
        let _ = writeln!(out, "{}: {}", make_path(&obj_rel), make_path(&rel));
        if unit.override_build_cmd && !unit.build_cmd.is_empty() {
            let cmd = unit.build_cmd.replace("<CRTAB>", "\n\t");
            let _ = writeln!(out, "\t{cmd}");
        } else if compile_as_cpp {
            let _ = writeln!(
                out,
                "\t$(CXX) -c {} -o {} $(CXXFLAGS) $(INCLUDES)",
                make_path(&rel),
                make_path(&obj_rel)
            );
        } else {
            let _ = writeln!(
                out,
                "\t$(CC) -c {} -o {} $(CFLAGS) $(INCLUDES)",
                make_path(&rel),
                make_path(&obj_rel)
            );
        }
        out.push('\n');
    }

    let _ = writeln!(out, "clean:\n\t$(RM) $(OBJ) $(BIN)");

    Ok(GeneratedMakefile {
        path: makefile_path,
        content: out,
    })
}

/// Writes the Makefile to disk, creating output directories.
pub fn write(project: &Project) -> std::io::Result<PathBuf> {
    let generated = generate(project).map_err(std::io::Error::other)?;
    if let Some(parent) = generated.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir = project.directory();
    let exe_dir = project.executable_path().parent().map(Path::to_path_buf);
    if let Some(d) = exe_dir {
        if !d.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(d);
        }
    }
    let obj_dir = if project.options.object_output.is_empty() {
        dir
    } else {
        paths::expand_file(&dir, &project.options.object_output)
    };
    let _ = std::fs::create_dir_all(obj_dir);
    std::fs::write(&generated.path, &generated.content)?;
    Ok(generated.path)
}

/// Errors raised while generating a Makefile.
#[derive(Debug, thiserror::Error)]
pub enum MakefileError {
    /// The project contains no compilable source file.
    #[error("project has no compilable source files")]
    NoSources,
}

fn is_source(file_name: &str) -> bool {
    let ext = paths::ext_of(Path::new(file_name));
    matches!(ext.as_str(), ".c" | ".cpp" | ".cc" | ".cxx" | ".c++")
}

fn object_path(project: &Project, dir: &Path, rel: &Path) -> PathBuf {
    if project.options.object_output.is_empty() {
        return paths::change_ext(rel, "o");
    }
    let obj_dir = paths::expand_file(dir, &project.options.object_output);
    let name = paths::file_name(rel);
    obj_dir.join(paths::change_ext(Path::new(&name), "o"))
}

/// Escapes a path for GNU make: spaces become `\ `.
#[must_use]
pub fn make_path(path: &Path) -> String {
    path.to_string_lossy().replace(' ', "\\ ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, ProjectType, Unit};

    fn project_with(units: Vec<Unit>) -> Project {
        let mut p = Project::new("/tmp/proj/demo.dev");
        p.units = units;
        p
    }

    fn unit(name: &str, cpp: Option<bool>) -> Unit {
        Unit {
            file_name: name.to_string(),
            compile_cpp: cpp,
            ..Unit::default()
        }
    }

    #[test]
    fn generates_targets_for_c_project() {
        let project = project_with(vec![unit("main.c", Some(false))]);
        let mf = generate(&project).expect("makefile");
        assert!(mf.content.contains("all: $(BIN)"));
        assert!(mf.content.contains("$(CC) -c main.c -o obj/main.o"));
        assert!(mf.content.contains("clean:"));
    }

    #[test]
    fn generates_cpp_rule() {
        let project = project_with(vec![unit("main.cpp", Some(true))]);
        let mf = generate(&project).expect("makefile");
        assert!(mf.content.contains("$(CXX) -c main.cpp -o obj/main.o"));
    }

    #[test]
    fn static_library_uses_ar() {
        let mut project = project_with(vec![unit("main.c", Some(false))]);
        project.options.project_type = ProjectType::StaticLibrary;
        let mf = generate(&project).expect("makefile");
        assert!(mf.content.contains("ar rcs"));
    }

    #[test]
    fn shared_library_uses_shared_flag() {
        let mut project = project_with(vec![unit("main.cpp", Some(true))]);
        project.options.project_type = ProjectType::DynamicLibrary;
        let mf = generate(&project).expect("makefile");
        assert!(mf.content.contains("-shared"));
    }

    #[test]
    fn no_sources_is_error() {
        let project = project_with(vec![]);
        assert!(generate(&project).is_err());
    }

    #[test]
    fn headers_are_not_compiled() {
        let project = project_with(vec![unit("main.h", None), unit("main.c", Some(false))]);
        let mf = generate(&project).expect("makefile");
        assert!(!mf.content.contains("main.h -o"));
    }

    #[test]
    fn flags_follow_option_vector() {
        let mut project = project_with(vec![unit("main.c", Some(false))]);
        project.options.compiler_options = vec![0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        let mf = generate(&project).expect("makefile");
        assert!(mf.content.contains("CFLAGS = -O2"));
    }

    #[test]
    fn make_path_escapes_spaces() {
        assert_eq!(make_path(Path::new("a b/main.o")), "a\\ b/main.o");
    }
}
