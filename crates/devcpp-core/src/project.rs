//! The `.dev` project file format, as written and read by Embarcadero Dev-C++.
//!
//! Reference: `Source/Project.pas` in the Embarcadero/Dev-Cpp repository
//! (`TProject.LoadOptions`, `TProject.SaveOptions`, `TProject.Open`,
//! `TProject.SaveUnits`). The format is a Windows INI file:
//!
//! ```ini
//! [Project]
//! Name=Hello
//! Type=1
//! Compiler=gcc
//! CppCompiler=g++
//! ...
//!
//! [Unit1]
//! FileName=main.cpp
//! Folder=Sources
//! Compile=1
//! CompileCpp=1
//! Link=1
//! ```
//!
//! The parser below is deliberately compatible with Delphi's `TIniFile`
//! semantics used by the original: keys are case-insensitive, values may be
//! quoted, `;` starts a comment, and a missing key yields the supplied default.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths;

/// Project kind, matching `TProjType` in `Source/ProjectTypes.pas`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    /// `dptGUI` — Windows GUI application.
    Gui,
    /// `dptCon` — console application.
    Console,
    /// `dptStat` — static library.
    StaticLibrary,
    /// `dptDyn` — dynamic library (shared object on Linux).
    DynamicLibrary,
}

impl ProjectType {
    /// Numeric value used in the `.dev` file (`Project/Type`).
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Gui => 0,
            Self::Console => 1,
            Self::StaticLibrary => 2,
            Self::DynamicLibrary => 3,
        }
    }

    /// Parses the numeric `Project/Type` value.
    #[must_use]
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Gui,
            1 => Self::Console,
            2 => Self::StaticLibrary,
            _ => Self::DynamicLibrary,
        }
    }

    /// Human-readable name, as shown in the Dev-C++ "New Project" dialog.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Gui => "Windows GUI Application",
            Self::Console => "Console Application",
            Self::StaticLibrary => "Static Library",
            Self::DynamicLibrary => "Dynamic Link Library",
        }
    }
}

/// Version information block (`[VersionInfo]` section).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Major version number.
    pub major: u32,
    /// Minor version number.
    pub minor: u32,
    /// Release number.
    pub release: u32,
    /// Build number.
    pub build: u32,
    /// Windows language identifier.
    pub language_id: u32,
    /// Windows charset identifier.
    pub charset_id: u32,
    /// Company name shown in the file properties.
    pub company_name: String,
    /// File version string.
    pub file_version: String,
    /// File description.
    pub file_description: String,
    /// Internal module name.
    pub internal_name: String,
    /// Copyright notice.
    pub legal_copyright: String,
    /// Trademarks notice.
    pub legal_trademarks: String,
    /// Original file name.
    pub original_filename: String,
    /// Product name.
    pub product_name: String,
    /// Product version string.
    pub product_version: String,
    /// Whether the build number increments automatically.
    pub auto_inc_build_nr: bool,
    /// Whether product and file versions are kept in sync.
    pub sync_product: bool,
}

impl Default for VersionInfo {
    fn default() -> Self {
        Self {
            major: 0,
            minor: 1,
            release: 1,
            build: 1,
            language_id: 0x0409,
            charset_id: 0x04E4,
            company_name: String::new(),
            file_version: "0.1".to_string(),
            file_description: String::new(),
            internal_name: String::new(),
            legal_copyright: String::new(),
            legal_trademarks: String::new(),
            original_filename: String::new(),
            product_name: String::new(),
            product_version: String::new(),
            auto_inc_build_nr: false,
            sync_product: false,
        }
    }
}

/// One source unit of the project (`[UnitN]` section).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unit {
    /// Path relative to the project directory (or absolute), as stored in the file.
    pub file_name: String,
    /// Logical folder shown in the project tree.
    pub folder: String,
    /// Whether the unit is passed to the compiler.
    pub compile: bool,
    /// Whether the unit is compiled as C++ rather than C.
    /// `None` means "inherit from the project default" (`CompileCpp=2`).
    pub compile_cpp: Option<bool>,
    /// Whether the unit's object file is linked into the final binary.
    pub link: bool,
    /// Build priority (lower builds first).
    pub priority: i32,
    /// Overrides the generated build command for this unit.
    /// Whether the unit uses its own build command.
    pub override_build_cmd: bool,
    /// Custom build command for this unit.
    /// Custom build command for the unit.
    pub build_cmd: String,
    /// Whether the unit is compiled with the C++ compiler regardless of the
    /// project setting (legacy `CompileCpp` semantics).
    /// Whether the unit is compiled as C++.
    pub is_cpp: bool,
}

impl Default for Unit {
    fn default() -> Self {
        Self {
            file_name: String::new(),
            folder: String::new(),
            compile: true,
            compile_cpp: None,
            link: true,
            priority: 1000,
            override_build_cmd: false,
            build_cmd: String::new(),
            is_cpp: false,
        }
    }
}

impl Unit {
    /// Absolute path of the unit, resolved against the project directory.
    #[must_use]
    pub fn absolute_path(&self, project_dir: &Path) -> PathBuf {
        paths::expand_file(project_dir, &self.file_name)
    }
}

/// Project-wide options, mirroring `TProjOptions` in `Source/ProjectTypes.pas`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOptions {
    /// Kind of project.
    pub project_type: ProjectType,
    /// C compiler command.
    pub compiler_cmd: String,
    /// C++ compiler command.
    pub cpp_compiler_cmd: String,
    /// Linker driver command.
    pub linker_cmd: String,
    /// Object files added to the link line.
    /// Extra object files for the link line.
    pub obj_files: String,
    /// Libraries added to the link line.
    /// Libraries for the link line.
    pub libs: String,
    /// Include directories.
    pub includes: String,
    /// Private resource script (Windows `.rc`; unused on Linux).
    /// Private resource script (Windows only).
    pub private_resource: String,
    /// Resource include directories (Windows only).
    pub resource_includes: String,
    /// Additional makefile fragments.
    /// Extra Makefile fragments.
    pub make_includes: String,
    /// Whether the project compiles as C++ by default.
    pub use_gpp: bool,
    /// Output directory for the executable.
    /// Output directory of the executable.
    pub exe_output: String,
    /// Output directory for object files.
    /// Output directory of object files.
    pub object_output: String,
    /// Log output file.
    /// Log file path.
    pub log_output: String,
    /// Whether the build log is written to a file.
    pub log_output_enabled: bool,
    /// Whether the output name is overridden.
    pub override_output: bool,
    /// Overridden output name.
    pub override_output_name: String,
    /// Host application for DLL projects.
    pub host_application: String,
    /// Whether a custom Makefile is used.
    pub use_custom_makefile: bool,
    /// Path of the custom Makefile.
    pub custom_makefile: String,
    /// Command line arguments passed to the program at run time.
    /// Command line arguments passed to the program.
    pub cmd_line_args: String,
    /// Logical folders shown in the project tree.
    /// Logical folders of the project tree.
    pub folders: String,
    /// Whether version information is embedded.
    pub include_version_info: bool,
    /// Whether Windows XP themes are supported.
    pub support_xp_themes: bool,
    /// Index into the compiler set table.
    /// Index of the compiler set.
    pub compiler_set: i32,
    /// Per-option compiler switches (index => enabled).
    /// Per-switch compiler option flags.
    pub compiler_options: Vec<i32>,
    /// Version information block.
    pub version_info: VersionInfo,
}

impl Default for ProjectOptions {
    fn default() -> Self {
        Self {
            project_type: ProjectType::Console,
            compiler_cmd: "gcc".to_string(),
            cpp_compiler_cmd: "g++".to_string(),
            linker_cmd: "g++".to_string(),
            obj_files: String::new(),
            libs: String::new(),
            includes: String::new(),
            private_resource: String::new(),
            resource_includes: String::new(),
            make_includes: String::new(),
            use_gpp: false,
            exe_output: "bin".to_string(),
            object_output: "obj".to_string(),
            log_output: String::new(),
            log_output_enabled: false,
            override_output: false,
            override_output_name: String::new(),
            host_application: String::new(),
            use_custom_makefile: false,
            custom_makefile: String::new(),
            cmd_line_args: String::new(),
            folders: String::new(),
            include_version_info: false,
            support_xp_themes: false,
            compiler_set: 0,
            compiler_options: Vec::new(),
            version_info: VersionInfo::default(),
        }
    }
}

/// A Dev-C++ project: options plus units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Path of the `.dev` file itself.
    pub file_name: PathBuf,
    /// Display name (`Project/Name`).
    pub name: String,
    /// Icon file name (Windows-only, kept for format compatibility).
    pub icon: String,
    /// Format version (`Project/Ver`).
    pub ver: i32,
    /// Project-wide options.
    pub options: ProjectOptions,
    /// Source units of the project.
    pub units: Vec<Unit>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            file_name: PathBuf::new(),
            name: "Untitled".to_string(),
            icon: String::new(),
            ver: 2,
            options: ProjectOptions::default(),
            units: Vec::new(),
        }
    }
}

/// Error type for project (de)serialization.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// The `.dev` file could not be read or written.
    #[error("io error on {path}: {source}")]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file did not contain a `[Project]` section.
    #[error("{0} is not a Dev-C++ project file")]
    NotAProject(PathBuf),
}

/// A minimal, case-insensitive INI reader/writer matching Delphi `TIniFile`
/// behaviour closely enough for `.dev` round-tripping.
#[derive(Debug, Default, Clone)]
pub struct Ini {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Ini {
    /// Creates an empty INI document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses INI text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut ini = Self::new();
        let mut current = String::new();
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some(name) = rest.strip_suffix(']') {
                    current = name.trim().to_string();
                    ini.sections.entry(current.clone()).or_default();
                    continue;
                }
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                let value = unquote(value.trim());
                ini.sections
                    .entry(current.to_ascii_lowercase())
                    .or_default()
                    .insert(key, value);
            }
        }
        ini
    }

    /// Serializes back to INI text.
    #[must_use]
    pub fn to_string_ini(&self) -> String {
        let mut out = String::new();
        for (section, entries) in &self.sections {
            let _ = writeln!(out, "[{section}]");
            for (key, value) in entries {
                let _ = writeln!(out, "{key}={value}");
            }
            out.push('\n');
        }
        out
    }

    /// Reads a string value with a default.
    #[must_use]
    pub fn get_str(&self, section: &str, key: &str, default: &str) -> String {
        self.sections
            .get(&section.to_ascii_lowercase())
            .and_then(|s| s.get(&key.to_ascii_lowercase()))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    /// Reads an integer value with a default.
    #[must_use]
    pub fn get_int(&self, section: &str, key: &str, default: i32) -> i32 {
        self.get_str(section, key, "")
            .trim()
            .parse::<i32>()
            .unwrap_or(default)
    }

    /// Reads a boolean value with a default. Delphi's `ReadBool` treats
    /// `1`/`true`/`yes` as true, everything else as false.
    #[must_use]
    pub fn get_bool(&self, section: &str, key: &str, default: bool) -> bool {
        let raw = self.get_str(section, key, "");
        if raw.is_empty() {
            return default;
        }
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }

    /// Writes a string value.
    pub fn set_str(&mut self, section: &str, key: &str, value: &str) {
        self.sections
            .entry(section.to_ascii_lowercase())
            .or_default()
            .insert(key.to_ascii_lowercase(), value.to_string());
    }

    /// Writes an integer value.
    pub fn set_int(&mut self, section: &str, key: &str, value: i32) {
        self.set_str(section, key, &value.to_string());
    }

    /// Writes a boolean value as `1`/`0` (Delphi `WriteBool` convention).
    pub fn set_bool(&mut self, section: &str, key: &str, value: bool) {
        self.set_str(section, key, if value { "1" } else { "0" });
    }
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

impl Project {
    /// Creates an empty project rooted at `file_name`.
    #[must_use]
    pub fn new(file_name: impl Into<PathBuf>) -> Self {
        let file_name = file_name.into();
        let name = file_name
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        Self {
            file_name,
            name,
            ..Self::default()
        }
    }

    /// Directory containing the `.dev` file.
    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.file_name
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    /// Path of the generated Makefile (`<project>.Makefile` by default, exactly
    /// as Dev-C++ names it).
    #[must_use]
    pub fn makefile_name(&self) -> PathBuf {
        if self.options.use_custom_makefile && !self.options.custom_makefile.is_empty() {
            return self.directory().join(&self.options.custom_makefile);
        }
        let stem = self
            .file_name
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Makefile".to_string());
        self.directory().join(format!("{stem}.Makefile"))
    }

    /// Name of the produced executable (without directory), honouring
    /// `OverrideOutputName`.
    #[must_use]
    pub fn executable_name(&self) -> String {
        if self.options.override_output && !self.options.override_output_name.is_empty() {
            return paths::file_name(Path::new(&self.options.override_output_name));
        }
        let stem = self
            .file_name
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "a".to_string());
        match self.options.project_type {
            ProjectType::StaticLibrary => format!("lib{stem}.a"),
            ProjectType::DynamicLibrary => format!("lib{stem}.so"),
            _ => stem,
        }
    }

    /// Absolute path of the produced executable.
    #[must_use]
    pub fn executable_path(&self) -> PathBuf {
        if self.options.override_output && !self.options.override_output_name.is_empty() {
            let p = PathBuf::from(&self.options.override_output_name);
            if p.is_absolute() {
                return p;
            }
            return self.directory().join(p);
        }
        let dir = if self.options.exe_output.is_empty() {
            self.directory()
        } else {
            paths::expand_file(&self.directory(), &self.options.exe_output)
        };
        dir.join(self.executable_name())
    }

    /// Absolute path of the primary source file, used by the debugger as the
    /// program to launch.
    #[must_use]
    pub fn main_source(&self) -> Option<PathBuf> {
        self.units
            .iter()
            .find(|u| u.compile && !u.folder.eq_ignore_ascii_case("Resources"))
            .map(|u| u.absolute_path(&self.directory()))
    }

    /// Reads a `.dev` file from disk.
    ///
    /// # Errors
    /// Returns [`ProjectError::Io`] when the file cannot be read and
    /// [`ProjectError::NotAProject`] when no `[Project]` section is present.
    pub fn load(file_name: impl Into<PathBuf>) -> Result<Self, ProjectError> {
        let path = file_name.into();
        let text = std::fs::read_to_string(&path).map_err(|source| ProjectError::Io {
            path: path.clone(),
            source,
        })?;
        let mut project = Self::from_ini_text(&text, &path);
        if project.units.is_empty() && project.name.is_empty() {
            return Err(ProjectError::NotAProject(path));
        }
        project.file_name = path;
        Ok(project)
    }

    /// Parses `.dev` text.
    #[must_use]
    pub fn from_ini_text(text: &str, file_name: &Path) -> Self {
        let ini = Ini::parse(text);
        let mut project = Self::new(file_name.to_path_buf());

        project.name = ini.get_str("Project", "Name", &project.name);
        project.icon = ini.get_str("Project", "Icon", "");
        project.ver = ini.get_int("Project", "Ver", 2);

        let opts = &mut project.options;
        opts.project_type =
            ProjectType::from_u8(ini.get_int("Project", "Type", 1).clamp(0, 3) as u8);
        opts.compiler_cmd = ini.get_str("Project", "Compiler", "gcc");
        opts.cpp_compiler_cmd = ini.get_str("Project", "CppCompiler", "g++");
        opts.linker_cmd = ini.get_str("Project", "Linker", "");
        opts.obj_files = ini.get_str("Project", "ObjFiles", "");
        opts.libs = ini.get_str("Project", "Libs", "");
        opts.includes = ini.get_str("Project", "Includes", "");
        opts.private_resource = ini.get_str("Project", "PrivateResource", "");
        opts.resource_includes = ini.get_str("Project", "ResourceIncludes", "");
        opts.make_includes = ini.get_str("Project", "MakeIncludes", "");
        opts.use_gpp = ini.get_bool("Project", "IsCpp", false);
        opts.exe_output = ini.get_str("Project", "ExeOutput", "bin");
        opts.object_output = ini.get_str("Project", "ObjectOutput", "obj");
        opts.log_output = ini.get_str("Project", "LogOutput", "");
        opts.log_output_enabled = ini.get_bool("Project", "LogOutputEnabled", false);
        opts.override_output = ini.get_bool("Project", "OverrideOutput", false);
        opts.override_output_name = ini.get_str("Project", "OverrideOutputName", "");
        opts.host_application = ini.get_str("Project", "HostApplication", "");
        opts.use_custom_makefile = ini.get_bool("Project", "UseCustomMakefile", false);
        opts.custom_makefile = ini.get_str("Project", "CustomMakefile", "");
        opts.cmd_line_args = ini.get_str("Project", "CommandLine", "");
        opts.folders = ini.get_str("Project", "Folders", "");
        opts.include_version_info = ini.get_bool("Project", "IncludeVersionInfo", false);
        opts.support_xp_themes = ini.get_bool("Project", "SupportXPThemes", false);
        opts.compiler_set = ini.get_int("Project", "CompilerSet", 0);

        let settings = ini.get_str("Project", "CompilerSettings", "");
        opts.compiler_options = settings
            .split(',')
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect();

        let vi = &mut opts.version_info;
        vi.major = ini.get_int("VersionInfo", "Major", 0).max(0) as u32;
        vi.minor = ini.get_int("VersionInfo", "Minor", 1).max(0) as u32;
        vi.release = ini.get_int("VersionInfo", "Release", 1).max(0) as u32;
        vi.build = ini.get_int("VersionInfo", "Build", 1).max(0) as u32;
        vi.language_id = ini.get_int("VersionInfo", "LanguageID", 0x0409) as u32;
        vi.charset_id = ini.get_int("VersionInfo", "CharsetID", 0x04E4) as u32;
        vi.company_name = ini.get_str("VersionInfo", "CompanyName", "");
        vi.file_version = ini.get_str("VersionInfo", "FileVersion", "0.1");
        vi.file_description = ini.get_str("VersionInfo", "FileDescription", "");
        vi.internal_name = ini.get_str("VersionInfo", "InternalName", "");
        vi.legal_copyright = ini.get_str("VersionInfo", "LegalCopyright", "");
        vi.legal_trademarks = ini.get_str("VersionInfo", "LegalTrademarks", "");
        vi.original_filename = ini.get_str("VersionInfo", "OriginalFilename", "");
        vi.product_name = ini.get_str("VersionInfo", "ProductName", "");
        vi.product_version = ini.get_str("VersionInfo", "ProductVersion", "");
        vi.auto_inc_build_nr = ini.get_bool("VersionInfo", "AutoIncBuildNr", false);
        vi.sync_product = ini.get_bool("VersionInfo", "SyncProduct", false);

        let unit_count = ini.get_int("Project", "UnitCount", 0).max(0);
        for i in 0..unit_count {
            let section = format!("Unit{}", i + 1);
            let mut unit = Unit {
                file_name: ini.get_str(&section, "FileName", ""),
                folder: ini.get_str(&section, "Folder", ""),
                compile: ini.get_bool(&section, "Compile", true),
                link: ini.get_bool(&section, "Link", true),
                priority: ini.get_int(&section, "Priority", 1000),
                override_build_cmd: ini.get_bool(&section, "OverrideBuildCmd", false),
                build_cmd: ini.get_str(&section, "BuildCmd", ""),
                ..Unit::default()
            };
            // `CompileCpp=2` means "inherit the project default".
            let cpp_raw = ini.get_int(&section, "CompileCpp", 2);
            unit.compile_cpp = if cpp_raw == 2 {
                None
            } else {
                Some(cpp_raw != 0)
            };
            unit.is_cpp = unit.compile_cpp.unwrap_or(opts.use_gpp);
            if unit.file_name.is_empty() {
                continue;
            }
            project.units.push(unit);
        }

        project
    }

    /// Serializes the project to `.dev` text.
    #[must_use]
    pub fn to_ini_text(&self) -> String {
        let mut ini = Ini::new();

        ini.set_str("Project", "Name", &self.name);
        ini.set_str("Project", "Icon", &self.icon);
        ini.set_int("Project", "Ver", self.ver);
        ini.set_int("Project", "Type", self.options.project_type.as_u8() as i32);
        ini.set_str("Project", "Compiler", &self.options.compiler_cmd);
        ini.set_str("Project", "CppCompiler", &self.options.cpp_compiler_cmd);
        ini.set_str("Project", "Linker", &self.options.linker_cmd);
        ini.set_str("Project", "ObjFiles", &self.options.obj_files);
        ini.set_str("Project", "Libs", &self.options.libs);
        ini.set_str("Project", "Includes", &self.options.includes);
        ini.set_str("Project", "PrivateResource", &self.options.private_resource);
        ini.set_str(
            "Project",
            "ResourceIncludes",
            &self.options.resource_includes,
        );
        ini.set_str("Project", "MakeIncludes", &self.options.make_includes);
        ini.set_bool("Project", "IsCpp", self.options.use_gpp);
        ini.set_str("Project", "ExeOutput", &self.options.exe_output);
        ini.set_str("Project", "ObjectOutput", &self.options.object_output);
        ini.set_str("Project", "LogOutput", &self.options.log_output);
        ini.set_bool(
            "Project",
            "LogOutputEnabled",
            self.options.log_output_enabled,
        );
        ini.set_bool("Project", "OverrideOutput", self.options.override_output);
        ini.set_str(
            "Project",
            "OverrideOutputName",
            &self.options.override_output_name,
        );
        ini.set_str("Project", "HostApplication", &self.options.host_application);
        ini.set_bool(
            "Project",
            "UseCustomMakefile",
            self.options.use_custom_makefile,
        );
        ini.set_str("Project", "CustomMakefile", &self.options.custom_makefile);
        ini.set_str("Project", "CommandLine", &self.options.cmd_line_args);
        ini.set_str("Project", "Folders", &self.options.folders);
        ini.set_bool(
            "Project",
            "IncludeVersionInfo",
            self.options.include_version_info,
        );
        ini.set_bool("Project", "SupportXPThemes", self.options.support_xp_themes);
        ini.set_int("Project", "CompilerSet", self.options.compiler_set);
        ini.set_str(
            "Project",
            "CompilerSettings",
            &self
                .options
                .compiler_options
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        ini.set_int("Project", "UnitCount", self.units.len() as i32);

        let vi = &self.options.version_info;
        ini.set_int("VersionInfo", "Major", vi.major as i32);
        ini.set_int("VersionInfo", "Minor", vi.minor as i32);
        ini.set_int("VersionInfo", "Release", vi.release as i32);
        ini.set_int("VersionInfo", "Build", vi.build as i32);
        ini.set_int("VersionInfo", "LanguageID", vi.language_id as i32);
        ini.set_int("VersionInfo", "CharsetID", vi.charset_id as i32);
        ini.set_str("VersionInfo", "CompanyName", &vi.company_name);
        ini.set_str("VersionInfo", "FileVersion", &vi.file_version);
        ini.set_str("VersionInfo", "FileDescription", &vi.file_description);
        ini.set_str("VersionInfo", "InternalName", &vi.internal_name);
        ini.set_str("VersionInfo", "LegalCopyright", &vi.legal_copyright);
        ini.set_str("VersionInfo", "LegalTrademarks", &vi.legal_trademarks);
        ini.set_str("VersionInfo", "OriginalFilename", &vi.original_filename);
        ini.set_str("VersionInfo", "ProductName", &vi.product_name);
        ini.set_str("VersionInfo", "ProductVersion", &vi.product_version);
        ini.set_bool("VersionInfo", "AutoIncBuildNr", vi.auto_inc_build_nr);
        ini.set_bool("VersionInfo", "SyncProduct", vi.sync_product);

        for (i, unit) in self.units.iter().enumerate() {
            let section = format!("Unit{}", i + 1);
            ini.set_str(&section, "FileName", &unit.file_name);
            ini.set_str(&section, "Folder", &unit.folder);
            ini.set_bool(&section, "Compile", unit.compile);
            ini.set_int(
                &section,
                "CompileCpp",
                unit.compile_cpp.map_or(2, i32::from),
            );
            ini.set_bool(&section, "Link", unit.link);
            ini.set_int(&section, "Priority", unit.priority);
            ini.set_bool(&section, "OverrideBuildCmd", unit.override_build_cmd);
            ini.set_str(&section, "BuildCmd", &unit.build_cmd);
        }

        ini.to_string_ini()
    }

    /// Writes the project back to its `.dev` file.
    ///
    /// # Errors
    /// Returns [`ProjectError::Io`] when the file cannot be written.
    pub fn save(&self) -> Result<(), ProjectError> {
        std::fs::write(&self.file_name, self.to_ini_text()).map_err(|source| ProjectError::Io {
            path: self.file_name.clone(),
            source,
        })
    }

    /// Adds a unit to the project.
    pub fn add_unit(&mut self, unit: Unit) {
        self.units.push(unit);
    }

    /// Removes a unit by index.
    pub fn remove_unit(&mut self, index: usize) -> Option<Unit> {
        (index < self.units.len()).then(|| self.units.remove(index))
    }

    /// Units grouped by their logical folder, for the project tree view.
    #[must_use]
    pub fn folder_map(&self) -> BTreeMap<String, Vec<&Unit>> {
        let mut map: BTreeMap<String, Vec<&Unit>> = BTreeMap::new();
        for unit in &self.units {
            map.entry(unit.folder.clone()).or_default().push(unit);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SAMPLE: &str = "\
[Project]
Name=Hello
Type=1
Compiler=gcc
CppCompiler=g++
IsCpp=0
ExeOutput=bin
ObjectOutput=obj
UnitCount=2

[Unit1]
FileName=main.cpp
Folder=Sources
Compile=1
CompileCpp=1
Link=1

[Unit2]
FileName=util.c
Folder=Sources
Compile=1
CompileCpp=0
Link=1

[VersionInfo]
Major=1
Minor=2
FileVersion=1.2
";

    #[test]
    fn parses_units_and_options() {
        let project = Project::from_ini_text(SAMPLE, Path::new("/tmp/hello.dev"));
        assert_eq!(project.name, "Hello");
        assert_eq!(project.options.project_type, ProjectType::Console);
        assert_eq!(project.options.use_gpp, false);
        assert_eq!(project.units.len(), 2);
        assert_eq!(project.units[0].file_name, "main.cpp");
        assert_eq!(project.units[0].compile_cpp, Some(true));
        assert_eq!(project.units[1].compile_cpp, Some(false));
        assert_eq!(project.options.version_info.major, 1);
        assert_eq!(project.options.version_info.file_version, "1.2");
    }

    #[test]
    fn compile_cpp_two_means_inherit() {
        let text = SAMPLE.replace(
            "CompileCpp=1\nLink=1\n\n[Unit2]",
            "CompileCpp=2\nLink=1\n\n[Unit2]",
        );
        let project = Project::from_ini_text(&text, Path::new("/tmp/hello.dev"));
        assert_eq!(project.units[0].compile_cpp, None);
    }

    #[test]
    fn round_trips_through_ini() {
        let project = Project::from_ini_text(SAMPLE, Path::new("/tmp/hello.dev"));
        let text = project.to_ini_text();
        let again = Project::from_ini_text(&text, Path::new("/tmp/hello.dev"));
        assert_eq!(project, again);
    }

    #[test]
    fn executable_name_per_type() {
        let mut project = Project::new("/tmp/hello.dev");
        assert_eq!(project.executable_name(), "hello");
        project.options.project_type = ProjectType::StaticLibrary;
        assert_eq!(project.executable_name(), "libhello.a");
        project.options.project_type = ProjectType::DynamicLibrary;
        assert_eq!(project.executable_name(), "libhello.so");
    }

    #[test]
    fn ini_keys_are_case_insensitive() {
        let ini = Ini::parse("[Project]\nNAME=Foo\n");
        assert_eq!(ini.get_str("project", "name", ""), "Foo");
    }

    #[test]
    fn ini_quotes_and_comments() {
        let ini = Ini::parse("[Project]\n; comment\nName=\"Quoted\"\n");
        assert_eq!(ini.get_str("Project", "Name", ""), "Quoted");
    }

    #[test]
    fn ini_bool_parsing() {
        let ini = Ini::parse("[S]\nA=1\nB=true\nC=0\nD=\n");
        assert!(ini.get_bool("S", "A", false));
        assert!(ini.get_bool("S", "B", false));
        assert!(!ini.get_bool("S", "C", true));
        assert!(ini.get_bool("S", "D", true));
    }

    #[test]
    fn folder_map_groups_units() {
        let project = Project::from_ini_text(SAMPLE, Path::new("/tmp/hello.dev"));
        let map = project.folder_map();
        assert_eq!(map["Sources"].len(), 2);
    }
}
