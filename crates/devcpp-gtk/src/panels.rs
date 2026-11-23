//! The IDE's side and bottom panels: project tree, compiler output, debug
//! information and the class browser.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

use devcpp_core::debugger::{Breakpoint, DebugEvent, StackFrame, Variable};
use devcpp_core::diagnostics::{Diagnostic, Severity};
use devcpp_core::project::Project;
use devcpp_core::symbols::SymbolIndex;

/// The project tree panel.
#[allow(dead_code)]
pub struct ProjectPanel {
    /// The scrolled tree view.
    pub widget: gtk::ScrolledWindow,
    /// The tree store holding the project structure.
    pub store: gtk::TreeStore,
    /// The tree view.
    pub view: gtk::TreeView,
}

impl ProjectPanel {
    /// Creates an empty project panel.
    #[must_use]
    pub fn new() -> Self {
        let store = gtk::TreeStore::new(&[glib::GString::static_type()]);
        let view = gtk::TreeView::with_model(&store);
        view.set_headers_visible(false);
        view.set_enable_search(true);

        let renderer = gtk::CellRendererText::new();
        let column = gtk::TreeViewColumn::new();
        column.pack_start(&renderer, true);
        column.add_attribute(&renderer, "text", 0);
        view.append_column(&column);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_size_request(220, -1);

        Self {
            widget: scroller,
            store,
            view,
        }
    }

    /// Rebuilds the tree from the project model.
    pub fn set_project(&self, project: &Project) {
        self.store.clear();
        let root = self.store.append(None);
        self.store.set(&root, &[(0, &project.name)]);
        for (folder, units) in project.folder_map() {
            let label = if folder.is_empty() {
                "Sources".to_string()
            } else {
                folder.clone()
            };
            let folder_item = self.store.append(Some(&root));
            self.store.set(&folder_item, &[(0, &label)]);
            for unit in units {
                let item = self.store.append(Some(&folder_item));
                self.store.set(&item, &[(0, &unit.file_name)]);
            }
        }
        self.view.expand_all();
    }
}

/// The compiler output panel.
pub struct OutputPanel {
    /// The scrolled text view holding the log.
    pub widget: gtk::ScrolledWindow,
    /// The text view holding the log.
    pub view: gtk::TextView,
    /// The text buffer.
    pub buffer: gtk::TextBuffer,
    /// Diagnostics collected during the build as `(log line, diagnostic)`.
    ///
    /// The line is the 0-based buffer line the diagnostic starts on, so a
    /// click on the log can jump to the matching editor location.
    pub diagnostics: Rc<RefCell<Vec<(i32, Diagnostic)>>>,
}

impl OutputPanel {
    /// Creates an empty output panel.
    #[must_use]
    pub fn new() -> Self {
        let buffer = gtk::TextBuffer::new(None);
        let view = gtk::TextView::with_buffer(&buffer);
        view.set_editable(false);
        view.set_monospace(true);
        view.set_cursor_visible(false);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();

        Self {
            widget: scroller,
            view,
            buffer,
            diagnostics: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Appends a line to the log.
    pub fn append(&self, text: &str) {
        let mut end = self.buffer.end_iter();
        self.buffer.insert(&mut end, text);
        let mut end = self.buffer.end_iter();
        self.buffer.insert(&mut end, "\n");
    }

    /// Clears the log and the diagnostic list.
    pub fn clear(&self) {
        self.buffer.set_text("");
        self.diagnostics.borrow_mut().clear();
    }

    /// Records a diagnostic and writes it to the log.
    pub fn add_diagnostic(&self, diagnostic: &Diagnostic) {
        let location = diagnostic
            .location()
            .unwrap_or_else(|| "<unknown>".to_string());
        let label = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Info => "info",
        };
        // The buffer line where this diagnostic is written, for click-to-jump.
        let line = self.buffer.line_count().saturating_sub(1);
        self.append(&format!("{location}: {label}: {}", diagnostic.message));
        self.diagnostics
            .borrow_mut()
            .push((line, diagnostic.clone()));
    }

    /// The diagnostic at `index`, if any.
    #[must_use]
    pub fn diagnostic(&self, index: usize) -> Option<Diagnostic> {
        self.diagnostics.borrow().get(index).map(|(_, d)| d.clone())
    }

    /// The diagnostic that starts on `line`, if any.
    #[must_use]
    pub fn diagnostic_at_line(&self, line: i32) -> Option<Diagnostic> {
        self.diagnostics
            .borrow()
            .iter()
            .find(|(l, _)| *l == line)
            .map(|(_, d)| d.clone())
    }
}

/// The debug panel: breakpoints, stack and variables.
#[allow(dead_code)]
pub struct DebugPanel {
    /// The scrolled container.
    pub widget: gtk::ScrolledWindow,
    /// The text buffer holding the debug log.
    pub buffer: gtk::TextBuffer,
    /// The stack view.
    pub stack_view: gtk::TreeView,
    /// The stack store.
    pub stack_store: gtk::ListStore,
    /// The variables view.
    pub vars_view: gtk::TreeView,
    /// The variables store.
    pub vars_store: gtk::ListStore,
    /// The breakpoints view.
    pub bp_view: gtk::TreeView,
    /// The breakpoints store.
    pub bp_store: gtk::ListStore,
}

impl DebugPanel {
    /// Creates an empty debug panel.
    #[must_use]
    pub fn new() -> Self {
        let buffer = gtk::TextBuffer::new(None);
        let log = gtk::TextView::with_buffer(&buffer);
        log.set_editable(false);
        log.set_monospace(true);

        let stack_store =
            gtk::ListStore::new(&[glib::GString::static_type(), glib::GString::static_type()]);
        let stack_view = gtk::TreeView::with_model(&stack_store);
        add_text_column(&stack_view, "Function", 0);
        add_text_column(&stack_view, "Location", 1);

        let vars_store = gtk::ListStore::new(&[
            glib::GString::static_type(),
            glib::GString::static_type(),
            glib::GString::static_type(),
        ]);
        let vars_view = gtk::TreeView::with_model(&vars_store);
        add_text_column(&vars_view, "Name", 0);
        add_text_column(&vars_view, "Value", 1);
        add_text_column(&vars_view, "Type", 2);

        let bp_store =
            gtk::ListStore::new(&[glib::GString::static_type(), glib::GString::static_type()]);
        let bp_view = gtk::TreeView::with_model(&bp_store);
        add_text_column(&bp_view, "File", 0);
        add_text_column(&bp_view, "Line", 1);

        let notebook = gtk::Notebook::builder().hexpand(true).vexpand(true).build();
        let stack_scroll = gtk::ScrolledWindow::builder().child(&stack_view).build();
        let vars_scroll = gtk::ScrolledWindow::builder().child(&vars_view).build();
        let bp_scroll = gtk::ScrolledWindow::builder().child(&bp_view).build();
        let log_scroll = gtk::ScrolledWindow::builder().child(&log).build();
        notebook.append_page(&stack_scroll, Some(&gtk::Label::new(Some("Call Stack"))));
        notebook.append_page(&vars_scroll, Some(&gtk::Label::new(Some("Locals"))));
        notebook.append_page(&bp_scroll, Some(&gtk::Label::new(Some("Breakpoints"))));
        notebook.append_page(&log_scroll, Some(&gtk::Label::new(Some("Debug Console"))));

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&notebook)
            .build();

        Self {
            widget: scroller,
            buffer,
            stack_view,
            stack_store,
            vars_view,
            vars_store,
            bp_view,
            bp_store,
        }
    }

    /// Appends a line to the debug console.
    pub fn append(&self, text: &str) {
        let mut end = self.buffer.end_iter();
        self.buffer.insert(&mut end, text);
        let mut end = self.buffer.end_iter();
        self.buffer.insert(&mut end, "\n");
    }

    /// Shows the current stack.
    pub fn set_stack(&self, frames: &[StackFrame]) {
        self.stack_store.clear();
        for frame in frames {
            let function = frame.function.clone().unwrap_or_else(|| "??".into());
            let location = frame
                .location
                .as_ref()
                .map_or_else(String::new, |l| format!("{}:{}", l.file.display(), l.line));
            let iter = self.stack_store.append();
            self.stack_store
                .set(&iter, &[(0, &function), (1, &location)]);
        }
    }

    /// Shows the current variables.
    pub fn set_variables(&self, variables: &[Variable]) {
        self.vars_store.clear();
        for var in variables {
            let type_name = var.type_name.clone().unwrap_or_default();
            let iter = self.vars_store.append();
            self.vars_store
                .set(&iter, &[(0, &var.name), (1, &var.value), (2, &type_name)]);
        }
    }

    /// Shows the breakpoint list.
    pub fn set_breakpoints(&self, breakpoints: &[Breakpoint]) {
        self.bp_store.clear();
        for bp in breakpoints {
            let file = bp.location.file.display().to_string();
            let line = bp.location.line.to_string();
            let iter = self.bp_store.append();
            self.bp_store.set(&iter, &[(0, &file), (1, &line)]);
        }
    }

    /// Handles a debugger event.
    pub fn handle_event(&self, event: &DebugEvent) {
        match event {
            DebugEvent::Console(text) => self.append(text),
            DebugEvent::ProgramOutput(text) => self.append(text),
            DebugEvent::Stack(frames) => self.set_stack(frames),
            DebugEvent::Variables(vars) => self.set_variables(vars),
            DebugEvent::Breakpoints(bps) => self.set_breakpoints(bps),
            DebugEvent::Error(message) => self.append(&format!("error: {message}")),
            DebugEvent::State(_) | DebugEvent::Exited(_) => {}
        }
    }
}

/// The class browser panel.
#[allow(dead_code)]
pub struct ClassPanel {
    /// The scrolled container.
    pub widget: gtk::ScrolledWindow,
    /// The tree store.
    pub store: gtk::TreeStore,
    /// The tree view.
    pub view: gtk::TreeView,
}

impl ClassPanel {
    /// Creates an empty class browser.
    #[must_use]
    pub fn new() -> Self {
        let store = gtk::TreeStore::new(&[glib::GString::static_type()]);
        let view = gtk::TreeView::with_model(&store);
        view.set_headers_visible(false);

        let renderer = gtk::CellRendererText::new();
        let column = gtk::TreeViewColumn::new();
        column.pack_start(&renderer, true);
        column.add_attribute(&renderer, "text", 0);
        view.append_column(&column);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroller.set_size_request(220, -1);

        Self {
            widget: scroller,
            store,
            view,
        }
    }

    /// Rebuilds the class browser from the symbol index.
    pub fn set_index(&self, index: &SymbolIndex) {
        self.store.clear();
        for class in index.classes() {
            let item = self.store.append(None);
            self.store.set(&item, &[(0, &class.name)]);
            for member in index.members_of(&class.name) {
                let child = self.store.append(Some(&item));
                self.store.set(&child, &[(0, &member.label())]);
            }
        }
        self.view.expand_all();
    }
}

fn add_text_column(view: &gtk::TreeView, title: &str, index: i32) {
    let renderer = gtk::CellRendererText::new();
    let column = gtk::TreeViewColumn::new();
    column.set_title(title);
    column.pack_start(&renderer, true);
    column.add_attribute(&renderer, "text", index);
    view.append_column(&column);
}

/// Shows a GTK error dialog.
pub fn show_error(parent: &gtk::ApplicationWindow, title: &str, message: &str) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk::MessageType::Error)
        .buttons(gtk::ButtonsType::Close)
        .title(title)
        .build();
    dialog.set_text(Some(message));
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

/// Shows a GTK information dialog.
pub fn show_info(parent: &gtk::ApplicationWindow, title: &str, message: &str) {
    let dialog = gtk::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .message_type(gtk::MessageType::Info)
        .buttons(gtk::ButtonsType::Close)
        .title(title)
        .build();
    dialog.set_text(Some(message));
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

/// The path of the file stored in a tree row.
#[must_use]
pub fn row_path(view: &gtk::TreeView, path: &gtk::TreePath) -> Option<PathBuf> {
    let model = view.model()?;
    let iter = model.iter(path)?;
    let value = model.get_value(&iter, 0);
    value.get::<String>().ok().map(PathBuf::from)
}

/// Replaces the status bar text.
pub fn set_status(widget: &gtk::Label, message: &str) {
    widget.set_text(message);
}
