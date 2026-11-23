//! Project symbol index and code-completion model.
//!
//! Dev-C++ provides a "Class Browser" (classes, methods, globals) and code
//! completion based on its own parser (`Source/CodeTool.pas`). The Rust clone
//! uses `tree-sitter` with the C and C++ grammars, which gives exact syntax
//! trees without a compiler round-trip.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Node, Parser, Tree};

/// Kind of a symbol, as shown in the Class Browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// `class` / `struct` / `union`.
    Class,
    /// A member function.
    Method,
    /// A free function.
    Function,
    /// A global variable.
    Variable,
    /// A structure member.
    Field,
    /// A `typedef` / `using` alias.
    TypeAlias,
    /// A preprocessor macro.
    Macro,
}

/// A symbol found in the project sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// Symbol name.
    pub name: String,
    /// Kind.
    pub kind: SymbolKind,
    /// Enclosing class or namespace, when applicable.
    pub parent: Option<String>,
    /// Source file.
    pub file: PathBuf,
    /// 1-based line.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// Inferred signature, e.g. `int add(int, int)`.
    pub signature: Option<String>,
}

impl Symbol {
    /// Label shown in the Class Browser tree.
    #[must_use]
    pub fn label(&self) -> String {
        match &self.signature {
            Some(sig) => format!("{} {}", self.name, sig),
            None => self.name.clone(),
        }
    }
}

/// The whole-project symbol index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolIndex {
    symbols: Vec<Symbol>,
}

impl SymbolIndex {
    /// Builds an index by parsing every file of `files`.
    #[must_use]
    pub fn build(files: &[PathBuf]) -> Self {
        let mut symbols = Vec::new();
        for file in files {
            let Ok(source) = std::fs::read_to_string(file) else {
                continue;
            };
            let Some(language) = language_for(file) else {
                continue;
            };
            let Some(mut parser) = parser_for(language) else {
                continue;
            };
            let Some(tree) = parser.parse(&source, None) else {
                continue;
            };
            collect(&tree, &source, file, None, &mut symbols);
        }
        symbols.sort_by(|a, b| {
            a.file
                .cmp(&b.file)
                .then(a.line.cmp(&b.line))
                .then(a.column.cmp(&b.column))
        });
        Self { symbols }
    }

    /// All indexed symbols.
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Symbols of the given kind.
    #[must_use]
    pub fn by_kind(&self, kind: SymbolKind) -> Vec<&Symbol> {
        self.symbols.iter().filter(|s| s.kind == kind).collect()
    }

    /// All class-like symbols.
    #[must_use]
    pub fn classes(&self) -> Vec<&Symbol> {
        self.by_kind(SymbolKind::Class)
    }

    /// All functions and methods.
    #[must_use]
    pub fn functions(&self) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
            .collect()
    }

    /// Members of a class.
    #[must_use]
    pub fn members_of(&self, class: &str) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.parent.as_deref() == Some(class))
            .collect()
    }

    /// Names of every known symbol, for completion.
    #[must_use]
    pub fn completion_candidates(&self) -> Vec<CompletionCandidate> {
        self.symbols
            .iter()
            .map(|s| CompletionCandidate {
                name: s.name.clone(),
                kind: s.kind,
                signature: s.signature.clone(),
                detail: s.parent.clone(),
            })
            .collect()
    }

    /// Names of all members of the class named `class`.
    #[must_use]
    pub fn members_named(&self, class: &str) -> Vec<CompletionCandidate> {
        self.members_of(class)
            .into_iter()
            .map(|s| CompletionCandidate {
                name: s.name.clone(),
                kind: s.kind,
                signature: s.signature.clone(),
                detail: Some(class.to_string()),
            })
            .collect()
    }
}

/// A completion suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionCandidate {
    /// Identifier to insert.
    pub name: String,
    /// Kind of symbol.
    pub kind: SymbolKind,
    /// Signature shown next to the name.
    pub signature: Option<String>,
    /// Owning class, when relevant.
    pub detail: Option<String>,
}

/// Returns the tree-sitter language for a file, based on its extension.
#[must_use]
pub fn language_for(path: &Path) -> Option<Language> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())?;
    match ext.as_str() {
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "cpp" | "cxx" | "cc" | "hpp" | "hh" | "hxx" => Some(tree_sitter_cpp::LANGUAGE.into()),
        _ => None,
    }
}

fn parser_for(language: Language) -> Option<Parser> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    Some(parser)
}

fn collect(tree: &Tree, source: &str, file: &Path, parent: Option<&str>, out: &mut Vec<Symbol>) {
    visit(tree.root_node(), source, file, parent, out);
}

fn visit(node: Node<'_>, source: &str, file: &Path, parent: Option<&str>, out: &mut Vec<Symbol>) {
    let mut current_parent = parent.map(ToString::to_string);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        match kind {
            "struct_specifier" | "union_specifier" | "enum_specifier" | "class_specifier" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = text(source, name_node);
                    let (line, column) = position(name_node);
                    out.push(Symbol {
                        name: name.clone(),
                        kind: SymbolKind::Class,
                        parent: current_parent.clone(),
                        file: file.to_path_buf(),
                        line,
                        column,
                        signature: None,
                    });
                    if let Some(body) = child.child_by_field_name("body") {
                        visit(body, source, file, Some(&name), out);
                    }
                    continue;
                }
            }
            "function_definition" | "declaration" | "function_declaration" => {
                if let Some(declarator) = child.child_by_field_name("declarator") {
                    let (name, signature) = describe_function(source, declarator);
                    if !name.is_empty() {
                        let is_method = current_parent.is_some();
                        let (line, column) = position(declarator);
                        out.push(Symbol {
                            name,
                            kind: if is_method {
                                SymbolKind::Method
                            } else {
                                SymbolKind::Function
                            },
                            parent: current_parent.clone(),
                            file: file.to_path_buf(),
                            line,
                            column,
                            signature,
                        });
                    }
                }
            }
            "type_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = text(source, name_node);
                    let (line, column) = position(name_node);
                    out.push(Symbol {
                        name,
                        kind: SymbolKind::TypeAlias,
                        parent: current_parent.clone(),
                        file: file.to_path_buf(),
                        line,
                        column,
                        signature: None,
                    });
                }
            }
            "alias_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = text(source, name_node);
                    let (line, column) = position(name_node);
                    out.push(Symbol {
                        name,
                        kind: SymbolKind::TypeAlias,
                        parent: current_parent.clone(),
                        file: file.to_path_buf(),
                        line,
                        column,
                        signature: None,
                    });
                }
            }
            "preproc_def" | "preproc_function_def" => {
                let name = child
                    .child_by_field_name("name")
                    .map_or_else(String::new, |n| text(source, n));
                if !name.is_empty() {
                    let (line, column) = position(child);
                    out.push(Symbol {
                        name,
                        kind: SymbolKind::Macro,
                        parent: current_parent.clone(),
                        file: file.to_path_buf(),
                        line,
                        column,
                        signature: None,
                    });
                }
            }
            "field_declaration" => {
                let mut decl_cursor = child.walk();
                for decl in child.children_by_field_name("declarator", &mut decl_cursor) {
                    let name = last_identifier(source, decl);
                    if !name.is_empty() {
                        let (line, column) = position(decl);
                        out.push(Symbol {
                            name,
                            kind: SymbolKind::Field,
                            parent: current_parent.clone(),
                            file: file.to_path_buf(),
                            line,
                            column,
                            signature: None,
                        });
                    }
                }
            }
            "init_declarator" => {
                if let Some(name_node) = child.child_by_field_name("declarator") {
                    let name = last_identifier(source, name_node);
                    if !name.is_empty() {
                        let (line, column) = position(name_node);
                        out.push(Symbol {
                            name,
                            kind: SymbolKind::Variable,
                            parent: current_parent.clone(),
                            file: file.to_path_buf(),
                            line,
                            column,
                            signature: None,
                        });
                    }
                }
            }
            "namespace_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    current_parent = Some(text(source, name_node));
                }
            }
            _ => {}
        }

        visit(child, source, file, current_parent.as_deref(), out);
    }
}

fn text(source: &str, node: Node<'_>) -> String {
    source
        .get(node.byte_range())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn position(node: Node<'_>) -> (u32, u32) {
    let p = node.start_position();
    (p.row as u32 + 1, p.column as u32 + 1)
}

fn last_identifier(source: &str, node: Node<'_>) -> String {
    let mut current = node;
    while let Some(child) = current.child(0) {
        current = child;
    }
    text(source, current)
}

/// Extracts a function name and a best-effort signature.
fn describe_function(source: &str, declarator: Node<'_>) -> (String, Option<String>) {
    let mut name = String::new();
    let mut params: Option<String> = None;
    let mut return_type: Option<String> = None;
    let mut node = Some(declarator);

    while let Some(cur) = node {
        match cur.kind() {
            "function_declarator" => {
                if let Some(p) = cur.child_by_field_name("parameters") {
                    params = Some(text(source, p));
                }
                node = cur.child_by_field_name("declarator");
            }
            "identifier"
            | "field_identifier"
            | "type_identifier"
            | "qualified_identifier"
            | "operator_name"
            | "destructor_name"
            | "template_function" => {
                name = text(source, cur);
                node = None;
            }
            "pointer_declarator" | "reference_declarator" | "parenthesized_declarator" => {
                node = cur.child_by_field_name("declarator");
            }
            _ => {
                node = cur.child(0);
            }
        }
    }

    if let Some(parent) = declarator.parent() {
        if parent.kind() == "function_definition" || parent.kind() == "declaration" {
            if let Some(t) = parent.child_by_field_name("type") {
                return_type = Some(text(source, t));
            }
        }
    }

    let signature = match (return_type, params) {
        (Some(r), Some(p)) => Some(format!("{r} {p}")),
        (None, Some(p)) => Some(p),
        _ => None,
    };
    (name, signature)
}

/// Computes completion candidates for a cursor position.
///
/// `prefix` is the identifier fragment before the cursor, `context` is the
/// text preceding the cursor (used to detect `obj->` and `obj.` member
/// access).
#[must_use]
pub fn complete(index: &SymbolIndex, prefix: &str, context: &str) -> Vec<CompletionCandidate> {
    let mut pool: Vec<CompletionCandidate> = Vec::new();

    if let Some(receiver) = member_access_receiver(context) {
        if let Some(class) = find_type_of(index, &receiver) {
            pool.extend(index.members_named(&class));
        } else {
            for s in index.symbols() {
                if s.kind == SymbolKind::Method || s.kind == SymbolKind::Field {
                    pool.push(CompletionCandidate {
                        name: s.name.clone(),
                        kind: s.kind,
                        signature: s.signature.clone(),
                        detail: s.parent.clone(),
                    });
                }
            }
        }
    } else {
        pool.extend(index.completion_candidates());
    }

    let lower = prefix.to_ascii_lowercase();
    let mut seen = BTreeSet::new();
    pool.retain(|c| seen.insert(c.name.clone()));
    pool.sort_by(|a, b| {
        let a_starts = a.name.to_ascii_lowercase().starts_with(&lower);
        let b_starts = b.name.to_ascii_lowercase().starts_with(&lower);
        b_starts.cmp(&a_starts).then_with(|| a.name.cmp(&b.name))
    });
    pool.into_iter()
        .filter(|c| prefix.is_empty() || c.name.to_ascii_lowercase().starts_with(&lower))
        .collect()
}

/// Detects `receiver->` or `receiver.` at the end of `context`.
#[must_use]
pub fn member_access_receiver(context: &str) -> Option<String> {
    let trimmed = context.trim_end();
    let arrow = trimmed.rfind("->");
    let dot = trimmed.rfind('.');
    let idx = match (arrow, dot) {
        (Some(a), Some(d)) if a > d => a,
        (Some(_), Some(d)) => d,
        (Some(a), None) => a,
        (None, Some(d)) => d,
        (None, None) => return None,
    };
    let receiver = &trimmed[..idx];
    receiver
        .rsplit(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|s| !s.is_empty())
        .map(ToString::to_string)
}

/// Finds the class a variable belongs to, using field declarations.
#[must_use]
pub fn find_type_of(index: &SymbolIndex, variable: &str) -> Option<String> {
    index
        .symbols()
        .iter()
        .find(|s| s.kind == SymbolKind::Variable && s.name == variable)
        .and_then(|s| s.signature.clone())
}

/// Groups symbols for the Class Browser tree.
#[must_use]
pub fn class_browser_tree(index: &SymbolIndex) -> BTreeMap<String, Vec<&Symbol>> {
    let mut map: BTreeMap<String, Vec<&Symbol>> = BTreeMap::new();
    for class in index.classes() {
        map.entry(class.name.clone()).or_default();
    }
    for sym in index.symbols() {
        if let Some(parent) = &sym.parent {
            map.entry(parent.clone()).or_default().push(sym);
        }
    }
    map
}

/// Shared parser instance for repeated use.
#[must_use]
pub fn shared_parser() -> Arc<Parser> {
    Arc::new(parser_for(tree_sitter_c::LANGUAGE.into()).expect("C parser"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const CODE: &str = r#"
#include <stdio.h>
#define MAX 10

typedef struct Point {
    int x;
    int y;
    int length(Point* p);
} Point;

class Counter {
public:
    int value;
    void increment();
    int get() const;
};

static int helper(int a, int b) { return a + b; }

int global_counter = 0;

int main(void) {
    Point p;
    Counter c;
    return helper(1, 2);
}
"#;

    fn temp_source() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("main.cpp");
        std::fs::write(&file, CODE).expect("write");
        (dir, file)
    }

    #[test]
    fn finds_class_and_members() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        let classes = index.classes();
        assert!(classes.iter().any(|c| c.name == "Point"), "Point missing");
        assert!(
            classes.iter().any(|c| c.name == "Counter"),
            "Counter missing"
        );
        let members = index.members_of("Counter");
        assert!(
            members.iter().any(|m| m.name == "increment"),
            "increment missing"
        );
        assert!(members.iter().any(|m| m.name == "value"), "value missing");
    }

    #[test]
    fn finds_functions_and_globals() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        let functions = index.functions();
        assert!(functions.iter().any(|f| f.name == "helper"));
        assert!(functions.iter().any(|f| f.name == "main"));
        assert!(index
            .symbols()
            .iter()
            .any(|s| s.name == "global_counter" && s.kind == SymbolKind::Variable));
    }

    #[test]
    fn finds_macro() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        assert!(index
            .symbols()
            .iter()
            .any(|s| s.name == "MAX" && s.kind == SymbolKind::Macro));
    }

    #[test]
    fn completion_filters_by_prefix() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        let candidates = complete(&index, "help", "");
        assert!(candidates.iter().any(|c| c.name == "helper"));
        assert!(!candidates.iter().any(|c| c.name == "main"));
    }

    #[test]
    fn detects_member_access_receiver() {
        assert_eq!(member_access_receiver("p."), Some("p".to_string()));
        assert_eq!(member_access_receiver("p->"), Some("p".to_string()));
        assert_eq!(member_access_receiver("foo.bar."), Some("bar".to_string()));
        assert_eq!(member_access_receiver("nothing"), None);
    }

    #[test]
    fn class_browser_groups_members() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        let tree = class_browser_tree(&index);
        assert!(tree.contains_key("Counter"), "Counter missing in tree");
        assert!(tree["Counter"].iter().any(|s| s.name == "increment"));
    }

    #[test]
    fn symbols_carry_location() {
        let (_d, file) = temp_source();
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        let main = index
            .functions()
            .into_iter()
            .find(|f| f.name == "main")
            .expect("main");
        assert!(main.line > 1);
        assert_eq!(main.file, file);
    }

    #[test]
    fn parses_plain_c() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("main.c");
        std::fs::write(&file, "int add(int a, int b){return a+b;}\n").expect("write");
        let index = SymbolIndex::build(std::slice::from_ref(&file));
        assert!(index.functions().iter().any(|f| f.name == "add"));
    }
}
