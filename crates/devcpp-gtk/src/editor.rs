//! The source editor widget: a GtkSourceView with Dev-C++ style behaviour.
//!
//! Code completion is implemented with a plain GTK4 popover (list box) driven
//! by the headless completion engine in `devcpp-core`, because
//! `GtkSourceCompletionWords` only accepts a fixed word list at construction
//! time while the IDE needs the live project symbol index.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use sourceview::prelude::*;
use sourceview::LanguageManager;

use devcpp_core::completion;
use devcpp_core::symbols::{CompletionCandidate, SymbolIndex};

/// A single open source file.
pub struct Editor {
    /// The notebook page widget.
    pub widget: gtk::Box,
    /// The source view.
    pub view: sourceview::View,
    /// The buffer.
    pub buffer: sourceview::Buffer,
    /// Path of the file on disk.
    pub path: RefCell<PathBuf>,
    /// Whether the buffer has unsaved changes.
    pub modified: RefCell<bool>,
    /// The completion popup.
    completion: gtk::Popover,
    completion_list: gtk::ListBox,
    candidates: Rc<RefCell<Vec<CompletionCandidate>>>,
    index: Rc<RefCell<SymbolIndex>>,
}

impl Editor {
    /// Creates an editor for `path`, loading the file when it exists.
    pub fn new(path: &Path, index: Rc<RefCell<SymbolIndex>>) -> Self {
        let buffer = sourceview::Buffer::new(None);
        let view = sourceview::View::with_buffer(&buffer);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(true);
        view.set_auto_indent(true);
        view.set_insert_spaces_instead_of_tabs(true);
        view.set_tab_width(4);
        view.set_monospace(true);

        let scroller = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&view)
            .build();
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        widget.append(&scroller);

        // Completion popup: a popover with a list of candidates.
        let completion_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        let completion_scroll = gtk::ScrolledWindow::builder()
            .max_content_height(240)
            .propagate_natural_width(true)
            .child(&completion_list)
            .build();
        let completion = gtk::Popover::builder()
            .child(&completion_scroll)
            .has_arrow(false)
            .build();

        let editor = Self {
            widget,
            view,
            buffer,
            path: RefCell::new(path.to_path_buf()),
            modified: RefCell::new(false),
            completion,
            completion_list,
            candidates: Rc::new(RefCell::new(Vec::new())),
            index,
        };
        editor.load();
        editor.install_completion();
        editor
    }

    /// Reads the file into the buffer.
    pub fn load(&self) {
        let path = self.path.borrow().clone();
        if let Ok(text) = std::fs::read_to_string(&path) {
            self.buffer.set_text(&text);
            self.modified.replace(false);
        }
    }

    /// Writes the buffer to disk.
    ///
    /// # Errors
    /// Returns an error when the file cannot be written.
    pub fn save(&self) -> std::io::Result<()> {
        let path = self.path.borrow().clone();
        std::fs::write(&path, self.text())?;
        self.modified.replace(false);
        Ok(())
    }

    /// Writes the buffer to `path` and makes it the editor's new location.
    ///
    /// # Errors
    /// Returns an error when the file cannot be written.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.text())?;
        self.modified.replace(false);
        self.path.replace(path.to_path_buf());
        Ok(())
    }

    /// The current text of the buffer.
    #[must_use]
    pub fn text(&self) -> String {
        self.buffer
            .text(&self.buffer.start_iter(), &self.buffer.end_iter(), false)
            .to_string()
    }

    /// The cursor line, 1-based.
    #[must_use]
    pub fn cursor_line(&self) -> u32 {
        let iter = self.buffer.iter_at_mark(&self.buffer.get_insert());
        (iter.line() + 1) as u32
    }

    /// Moves the cursor to `line` (1-based) and scrolls there.
    pub fn goto_line(&self, line: u32) {
        let target = line.saturating_sub(1);
        let Some(mut iter) = self.buffer.iter_at_line(target as i32) else {
            return;
        };
        self.buffer.place_cursor(&iter);
        self.view.scroll_to_iter(&mut iter, 0.25, false, 0.0, 0.0);
    }

    /// Sets the language of the buffer from the file extension.
    pub fn apply_language(&self) {
        let manager = LanguageManager::default();
        let ext = self
            .path
            .borrow()
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let language = match ext.as_str() {
            "c" | "h" => manager.language("c"),
            "cpp" | "cxx" | "cc" | "hpp" | "hh" => manager.language("cpp"),
            _ => None,
        };
        if let Some(language) = language {
            self.buffer.set_language(Some(&language));
        }
    }

    /// Computes the candidates for the current cursor position.
    #[must_use]
    pub fn current_candidates(&self) -> Vec<CompletionCandidate> {
        completion_candidates(&self.buffer, &self.index)
    }

    /// Shows the completion popup at the cursor.
    pub fn show_completion(&self) {
        let candidates = self.current_candidates();
        if candidates.is_empty() {
            self.completion.popdown();
            return;
        }
        self.update_candidates(&candidates);
        popup_completion(&self.view, &self.completion);
    }

    /// Replaces the candidate list shown in the popup.
    pub fn update_candidates(&self, candidates: &[CompletionCandidate]) {
        self.candidates.replace(candidates.to_vec());
        refill_completion_list(&self.completion_list, candidates);
    }

    fn install_completion(&self) {
        let list = self.completion_list.clone();
        let candidates = Rc::clone(&self.candidates);
        let editor_view = self.view.clone();
        let popover = self.completion.clone();

        list.connect_row_activated(move |_, row| {
            let index = row.index();
            let name = {
                let list = candidates.borrow();
                let Some(candidate) = list.get(index as usize) else {
                    return;
                };
                candidate.name.clone()
            };
            if let Ok(buffer) = editor_view.buffer().downcast::<sourceview::Buffer>() {
                insert_completion_in_buffer(&buffer, &name);
            }
            popover.popdown();
        });

        // Tab / Return accept the selected candidate, Escape closes.
        let key_list = list.clone();
        let key_popover = self.completion.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                key_popover.popdown();
                glib::Propagation::Stop
            } else if keyval == gdk::Key::Tab || keyval == gdk::Key::Return {
                if let Some(row) = key_list.selected_row() {
                    row.activate();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            } else {
                glib::Propagation::Proceed
            }
        });
        self.completion.add_controller(key);

        // Auto-popup after "." or "->", like Dev-C++'s member completion.
        let popup_view = self.view.clone();
        let popup_popover = self.completion.clone();
        let popup_list = self.completion_list.clone();
        let popup_candidates = Rc::clone(&self.candidates);
        let popup_index = Rc::clone(&self.index);
        let typed = gtk::EventControllerKey::new();
        typed.connect_key_released(move |_, _, _, _| {
            if !cursor_follows_member_access(&popup_view) {
                return;
            }
            let view = popup_view.clone();
            let popover = popup_popover.clone();
            let list = popup_list.clone();
            let candidates = Rc::clone(&popup_candidates);
            let index = Rc::clone(&popup_index);
            glib::idle_add_local_once(move || {
                if !cursor_follows_member_access(&view) {
                    return;
                }
                let Some(buffer) = view.buffer().downcast::<sourceview::Buffer>().ok() else {
                    return;
                };
                let found = completion_candidates(&buffer, &index);
                if found.is_empty() {
                    popover.popdown();
                    return;
                }
                refill_completion_list(&list, &found);
                candidates.replace(found);
                popup_completion(&view, &popover);
            });
        });
        self.view.add_controller(typed);
    }

    /// Applies the configured font.
    pub fn apply_font(&self, family: &str, size: u16) {
        let css = format!("textview {{ font-family: {family}; font-size: {size}pt; }}");
        let provider = gtk::CssProvider::new();
        provider.load_from_data(&css);
        let context = self.view.style_context();
        context.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    /// Enables or disables the right margin (used as a minimap placeholder).
    #[allow(dead_code)]
    pub fn set_minimap(&self, enabled: bool) {
        self.view.set_show_right_margin(enabled);
    }
}

/// Computes completion candidates for the current cursor position of `buffer`.
fn completion_candidates(
    buffer: &sourceview::Buffer,
    index: &Rc<RefCell<SymbolIndex>>,
) -> Vec<CompletionCandidate> {
    let iter = buffer.iter_at_mark(&buffer.get_insert());
    let offset = iter.offset().max(0) as usize;
    let text = buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string();
    let ctx = completion::context_at(&text, offset, true);
    completion::candidates(&index.borrow(), &ctx)
}

/// Rebuilds the completion list rows.
fn refill_completion_list(list: &gtk::ListBox, candidates: &[CompletionCandidate]) {
    while let Some(row) = list.first_child() {
        list.remove(&row);
    }
    for candidate in candidates {
        let label = match &candidate.signature {
            Some(sig) => format!("{}  {}", candidate.name, sig),
            None => candidate.name.clone(),
        };
        let row = gtk::ListBoxRow::builder()
            .activatable(true)
            .child(&gtk::Label::new(Some(&label)))
            .build();
        list.append(&row);
    }
    if let Some(first) = list.first_child() {
        if let Some(row) = first.downcast_ref::<gtk::ListBoxRow>() {
            list.select_row(Some(row));
        }
    }
}

/// Whether the cursor sits right after `.` or `->`.
fn cursor_follows_member_access(view: &sourceview::View) -> bool {
    let Some(buffer) = view.buffer().downcast::<sourceview::Buffer>().ok() else {
        return false;
    };
    let iter = buffer.iter_at_mark(&buffer.get_insert());
    let Some(line_start) = buffer.iter_at_line(iter.line()) else {
        return false;
    };
    let before = buffer.text(&line_start, &iter, false).to_string();
    before.ends_with('.') || before.ends_with("->")
}

/// Shows the completion popup under the cursor.
fn popup_completion(view: &sourceview::View, popover: &gtk::Popover) {
    let (rect, _) = view.cursor_locations(None);
    popover.set_pointing_to(Some(&rect));
    popover.popup();
}

/// Inserts `name` at the cursor, replacing the word being typed.
fn insert_completion_in_buffer(buffer: &sourceview::Buffer, name: &str) {
    let iter = buffer.iter_at_mark(&buffer.get_insert());
    let Some(line_start) = buffer.iter_at_line(iter.line()) else {
        return;
    };
    let prefix_text = buffer.text(&line_start, &iter, false).to_string();
    let prefix_len = prefix_text
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .count();
    let mut start = buffer.iter_at_offset(iter.offset() - prefix_len as i32);
    let mut end = iter;
    buffer.delete(&mut start, &mut end);
    let mut cursor = buffer.iter_at_mark(&buffer.get_insert());
    buffer.insert(&mut cursor, name);
}
