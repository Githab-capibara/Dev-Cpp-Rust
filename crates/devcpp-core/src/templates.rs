//! Project templates.
//!
//! Dev-C++ ships templates as `.template` files (INI) plus `.txt` payloads
//! (`Templates/2-ConsoleApp.template`, `Templates/ConsoleApp_c.txt`, ...).
//! The clone provides the equivalent templates natively, and can also import
//! the original `.template` files for maximum compatibility.

use std::path::{Path, PathBuf};

use crate::project::{Project, ProjectType, Unit};

/// A project template offered in the "New Project" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Template name shown in the dialog.
    pub name: &'static str,
    /// Description shown below the name.
    pub description: &'static str,
    /// Kind of project created.
    pub project_type: ProjectType,
    /// Source files created (name, content, compiled as C++).
    pub files: &'static [(&'static str, &'static str, bool)],
    /// Whether the project compiles as C++ by default.
    pub default_cpp: bool,
}

/// The built-in template set, mirroring the Dev-C++ "New Project" dialog.
pub const TEMPLATES: &[Template] = &[
    Template {
        name: "Console Application",
        description: "A console application, for command line programs.",
        project_type: ProjectType::Console,
        files: &[("main.cpp", CONSOLE_CPP, true)],
        default_cpp: true,
    },
    Template {
        name: "Console Application (C)",
        description: "A console application written in plain C.",
        project_type: ProjectType::Console,
        files: &[("main.c", CONSOLE_C, false)],
        default_cpp: false,
    },
    Template {
        name: "Static Library",
        description: "A static library (.a) with public and private headers.",
        project_type: ProjectType::StaticLibrary,
        files: &[("main.cpp", STATIC_CPP, true), ("main.h", STATIC_H, false)],
        default_cpp: true,
    },
    Template {
        name: "Dynamic Link Library",
        description: "A shared library (.so) with public and private headers.",
        project_type: ProjectType::DynamicLibrary,
        files: &[
            ("main.cpp", DYNAMIC_CPP, true),
            ("main.h", DYNAMIC_H, false),
        ],
        default_cpp: true,
    },
    Template {
        name: "Empty Project",
        description: "An empty project with a single source file.",
        project_type: ProjectType::Console,
        files: &[("main.cpp", EMPTY_CPP, true)],
        default_cpp: true,
    },
];

const CONSOLE_CPP: &str = r#"#include <iostream>

/* run this program using the console pauser or add your own getch, system("pause") or input loop */
int main(int argc, char** argv) {
    std::cout << "Hello, World!" << std::endl;
    return 0;
}
"#;

const CONSOLE_C: &str = r#"#include <stdio.h>

int main(int argc, char** argv) {
    printf("Hello, World!\n");
    return 0;
}
"#;

const STATIC_CPP: &str = r#"#include "main.h"

int add(int a, int b) {
    return a + b;
}
"#;

const STATIC_H: &str = r#"#ifndef MAIN_H
#define MAIN_H

int add(int a, int b);

#endif
"#;

const DYNAMIC_CPP: &str = r#"#include "main.h"

extern "C" int multiply(int a, int b) {
    return a * b;
}
"#;

const DYNAMIC_H: &str = r#"#ifndef MAIN_H
#define MAIN_H

#ifdef __cplusplus
extern "C" {
#endif

int multiply(int a, int b);

#ifdef __cplusplus
}
#endif

#endif
"#;

const EMPTY_CPP: &str = r#"int main() {
    return 0;
}
"#;

/// Creates a project on disk from `template`.
///
/// Returns the path of the created `.dev` file.
///
/// # Errors
/// Returns an error when files cannot be written.
pub fn create(
    template: &Template,
    project_name: &str,
    directory: &Path,
) -> Result<PathBuf, TemplateError> {
    std::fs::create_dir_all(directory)?;

    let dev_path = directory.join(format!("{project_name}.dev"));
    let mut project = Project::new(&dev_path);
    project.name = project_name.to_string();
    project.options.project_type = template.project_type;
    project.options.use_gpp = template.default_cpp;
    project.options.compiler_cmd = "gcc".to_string();
    project.options.cpp_compiler_cmd = "g++".to_string();
    project.options.linker_cmd = if template.default_cpp { "g++" } else { "gcc" }.to_string();
    project.options.exe_output = "bin".to_string();
    project.options.object_output = "obj".to_string();

    for (name, content, is_cpp) in template.files {
        let path = directory.join(name);
        if path.exists() {
            return Err(TemplateError::FileExists(path));
        }
        std::fs::write(&path, content)?;
        project.add_unit(Unit {
            file_name: (*name).to_string(),
            folder: "Sources".to_string(),
            compile: true,
            compile_cpp: Some(*is_cpp),
            link: true,
            ..Unit::default()
        });
    }

    project
        .save()
        .map_err(|e| TemplateError::Io(e.to_string()))?;
    Ok(dev_path)
}

/// Looks up a template by display name.
#[must_use]
pub fn find_by_name(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// Errors of template creation.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// A file with the same name already exists.
    #[error("file already exists: {0}")]
    FileExists(PathBuf),
    /// I/O error.
    #[error("template error: {0}")]
    Io(String),
}

impl From<std::io::Error> for TemplateError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn creates_console_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let template = find_by_name("Console Application").expect("template");
        let dev = create(template, "demo", dir.path()).expect("create");
        assert!(dev.exists());
        assert!(dir.path().join("main.cpp").exists());

        let project = Project::load(&dev).expect("load");
        assert_eq!(project.name, "demo");
        assert_eq!(project.units.len(), 1);
        assert_eq!(project.units[0].file_name, "main.cpp");
        assert_eq!(project.options.project_type, ProjectType::Console);
    }

    #[test]
    fn creates_static_library_with_header() {
        let dir = tempfile::tempdir().expect("tempdir");
        let template = find_by_name("Static Library").expect("template");
        let dev = create(template, "mylib", dir.path()).expect("create");
        let project = Project::load(&dev).expect("load");
        assert_eq!(project.units.len(), 2);
        assert_eq!(project.options.project_type, ProjectType::StaticLibrary);
        assert!(dir.path().join("main.h").exists());
    }

    #[test]
    fn refuses_to_overwrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("main.cpp"), "x").expect("write");
        let template = find_by_name("Console Application").expect("template");
        assert!(create(template, "demo", dir.path()).is_err());
    }

    #[test]
    fn all_templates_have_files() {
        for template in TEMPLATES {
            assert!(!template.files.is_empty(), "{}", template.name);
        }
    }
}
