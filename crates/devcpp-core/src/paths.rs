//! Path helpers shared by the project model and the generator.
//!
//! The original Dev-C++ (Pascal, Windows) relies on VCL path helpers. This
//! module re-implements the small subset the IDE needs, in a portable way, and
//! keeps semantics identical to the original (relative paths are resolved
//! against the `.dev` file's directory, backslashes are tolerated on read).

use std::path::{Component, Path, PathBuf};

/// Normalizes a path that may use `/` or `\` as separator and may be relative
/// to `base`. Mirrors `ExpandFileto` in Dev-C++'s `Utils.pas`.
#[must_use]
pub fn expand_file(base: &Path, name: &str) -> PathBuf {
    if name.is_empty() {
        return base.to_path_buf();
    }
    let p = PathBuf::from(name.replace('\\', "/"));
    if p.is_absolute() {
        normalize(&p)
    } else {
        normalize(&base.join(p))
    }
}

/// Lexical normalization: drop `.` components, collapse `..` when possible.
/// No filesystem access (the file may not exist yet).
#[must_use]
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // pop only when the tail is a normal component
                let pops = out
                    .components()
                    .next_back()
                    .is_some_and(|c| matches!(c, Component::Normal(_)));
                if pops {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Returns `name` relative to `base` when possible, otherwise the absolute
/// path. Mirrors `ExtractRelativePath`.
#[must_use]
pub fn relative_path(base: &Path, name: &Path) -> PathBuf {
    let base = normalize(base);
    let name = normalize(name);
    let parent = base.parent().unwrap_or(&base);

    let base_comps: Vec<_> = parent.components().collect();
    let name_comps: Vec<_> = name.components().collect();

    let mut common = 0usize;
    for (a, b) in base_comps.iter().zip(name_comps.iter()) {
        if a == b {
            common += 1;
        } else {
            break;
        }
    }

    if common == 0 {
        return name;
    }

    let mut result = PathBuf::new();
    for _ in common..base_comps.len() {
        result.push("..");
    }
    for comp in &name_comps[common..] {
        result.push(comp.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

/// Replaces the file extension (`.cpp` -> `.o`).
#[must_use]
pub fn change_ext(path: &Path, ext: &str) -> PathBuf {
    let mut p = path.to_path_buf();
    p.set_extension(ext.trim_start_matches('.'));
    p
}

/// File name without any directory.
#[must_use]
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Lowercase extension, with the dot.
#[must_use]
pub fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|s| format!(".{}", s.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default()
}

/// Appends a trailing `/` to a makefile path (GNU make accepts both, the
/// original generator emits them, so we keep byte-identical output).
#[must_use]
pub fn with_trailing_slash(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return path.to_path_buf();
    }
    let mut s = path.as_os_str().to_string_lossy().to_string();
    if !s.ends_with('/') {
        s.push('/');
    }
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_relative() {
        let base = Path::new("/proj/dev");
        assert_eq!(
            expand_file(base, "src\\main.cpp"),
            PathBuf::from("/proj/dev/src/main.cpp")
        );
    }
    #[test]
    fn expand_absolute_wins() {
        let base = Path::new("/proj/dev/test.dev");
        assert_eq!(
            expand_file(base, "/usr/include/stdio.h"),
            PathBuf::from("/usr/include/stdio.h")
        );
    }

    #[test]
    fn normalize_dotdot() {
        assert_eq!(
            normalize(Path::new("/proj/dev/./src/../main.c")),
            PathBuf::from("/proj/dev/main.c")
        );
    }

    #[test]
    fn relative_to_project_dir() {
        let base = Path::new("/proj/dev/test.dev");
        let name = Path::new("/proj/dev/src/main.c");
        assert_eq!(relative_path(base, name), PathBuf::from("src/main.c"));
    }

    #[test]
    fn relative_up_and_over() {
        let base = Path::new("/proj/dev/test.dev");
        let name = Path::new("/proj/lib/x.c");
        assert_eq!(relative_path(base, name), PathBuf::from("../lib/x.c"));
    }

    #[test]
    fn change_extension() {
        assert_eq!(
            change_ext(Path::new("src/main.cpp"), "o"),
            PathBuf::from("src/main.o")
        );
    }

    #[test]
    fn ext_lowercase() {
        assert_eq!(ext_of(Path::new("A.CPP")), ".cpp");
        assert_eq!(ext_of(Path::new("Makefile")), "");
    }
}
