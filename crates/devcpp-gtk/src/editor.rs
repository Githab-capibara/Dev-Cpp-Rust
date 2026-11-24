//! The source editor widget: a GtkSourceView with Dev-C++ style behaviour.
//!
//! Code completion is implemented with a plain GTK4 popover (list box) driven
//! by the headless completion engine in `devcpp-core`, because
//! `GtkSourceCompletionWords` only accepts a fixed word list at construction
//! time while the IDE needs the live project symbol index.

use std::cell::{Cell, RefCell};
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
    /// The find/replace bar shown above the view.
    find_bar: gtk::Box,
    find_entry: gtk::Entry,
    find_info: gtk::Label,
    replace_row: gtk::Box,
    replace_entry: gtk::Entry,
    /// Live search context of the buffer (rebuilt when the bar shows).
    search: RefCell<Option<sourceview::SearchContext>>,
    /// Text snapshots for undo, oldest first.
    undo_history: RefCell<Vec<String>>,
    /// Text snapshots for redo, newest first.
    redo_history: RefCell<Vec<String>>,
    /// Baseline text of the last settled edit batch.
    last_sample: Rc<RefCell<String>>,
    /// Whether an edit batch is still accumulating changes.
    batch_open: Rc<Cell<bool>>,
}

impl Editor {
    /// Creates an editor for `path`, loading the file when it exists.
    pub fn new(path: &Path, index: Rc<RefCell<SymbolIndex>>) -> Rc<Self> {
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

        // Find and replace bar, hidden until Find/Replace is invoked.
        let find_entry = gtk::Entry::builder()
            .placeholder_text("Find")
            .hexpand(true)
            .build();
        let replace_entry = gtk::Entry::builder()
            .placeholder_text("Replace with")
            .hexpand(true)
            .build();
        let find_info = gtk::Label::new(None);
        let prev_button = gtk::Button::with_label("Previous");
        let next_button = gtk::Button::with_label("Next");
        let replace_button = gtk::Button::with_label("Replace");
        let replace_all_button = gtk::Button::with_label("Replace All");
        let close_button = gtk::Button::with_label("×");

        let find_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(6)
            .margin_end(6)
            .build();
        find_row.append(&find_entry);
        find_row.append(&prev_button);
        find_row.append(&next_button);
        find_row.append(&find_info);
        find_row.append(&close_button);
        let replace_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_bottom(4)
            .margin_start(6)
            .margin_end(6)
            .build();
        replace_row.append(&replace_entry);
        replace_row.append(&replace_button);
        replace_row.append(&replace_all_button);

        let find_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        find_bar.append(&find_row);
        find_bar.append(&replace_row);
        find_bar.set_visible(false);

        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        widget.append(&find_bar);
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

        let editor = Rc::new(Self {
            widget,
            view,
            buffer,
            path: RefCell::new(path.to_path_buf()),
            modified: RefCell::new(false),
            completion,
            completion_list,
            candidates: Rc::new(RefCell::new(Vec::new())),
            index,
            find_bar,
            find_entry,
            find_info,
            replace_row,
            replace_entry,
            search: RefCell::new(None),
            undo_history: RefCell::new(Vec::new()),
            redo_history: RefCell::new(Vec::new()),
            last_sample: Rc::new(RefCell::new(String::new())),
            batch_open: Rc::new(Cell::new(false)),
        });
        editor.load();
        editor.last_sample.replace(editor.text());
        editor.install_completion();
        Self::install_find_bar(
            &editor,
            &prev_button,
            &next_button,
            &replace_button,
            &replace_all_button,
            &close_button,
        );
        Self::install_undo(&editor);
        Self::install_change_hub(&editor);
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

    /// Pushes `text` onto the undo stack unless it duplicates the newest
    /// snapshot. Capped so a long session cannot grow without bound.
    fn push_undo_snapshot(&self, text: &str) {
        let mut history = self.undo_history.borrow_mut();
        if history.last().is_some_and(|last| last == text) {
            return;
        }
        history.push(text.to_string());
        if history.len() > UNDO_LIMIT {
            history.remove(0);
        }
    }

    /// Records the current text as a snapshot for later undo. Called by
    /// clipboard actions that bypass the ordinary keystroke path.
    pub fn snapshot_undo(&self) {
        let text = self.text();
        let mut history = self.undo_history.borrow_mut();
        if history.last().is_none_or(|last| *last != text) {
            history.push(text);
            if history.len() > UNDO_LIMIT {
                history.remove(0);
            }
        }
    }

    /// Restores the state before the last edit batch, if any.
    pub fn undo(&self) {
        let Some(previous) = self.undo_history.borrow_mut().pop() else {
            return;
        };
        let current = self.text();
        if current == previous {
            return;
        }
        self.batch_open.set(false);
        let mut redo = self.redo_history.borrow_mut();
        redo.push(current);
        if redo.len() > UNDO_LIMIT {
            redo.remove(0);
        }
        drop(redo);
        self.replace_text(&previous);
        self.last_sample.replace(previous);
    }

    /// Re-applies the state undone by [`Self::undo`].
    pub fn redo(&self) {
        let Some(next) = self.redo_history.borrow_mut().pop() else {
            return;
        };
        let current = self.text();
        if current == next {
            return;
        }
        self.batch_open.set(false);
        let mut history = self.undo_history.borrow_mut();
        history.push(current);
        if history.len() > UNDO_LIMIT {
            history.remove(0);
        }
        drop(history);
        self.replace_text(&next);
        self.last_sample.replace(next);
    }

    /// Replaces the whole buffer content with `text`.
    fn replace_text(&self, text: &str) {
        let mut start = self.buffer.start_iter();
        let mut end = self.buffer.end_iter();
        self.buffer.delete(&mut start, &mut end);
        let mut cursor = self.buffer.start_iter();
        self.buffer.insert(&mut cursor, text);
        self.modified.replace(true);
    }

    /// Ctrl+Z / Ctrl+Y (and Shift+Ctrl+Z): undo/redo dispatch.
    ///
    /// Snapshots come from [`Self::install_change_hub`] on real buffer
    /// changes, because on Wayland typed characters often arrive as text
    /// commits, and a key controller alone would miss every insertion.
    pub fn install_undo(me: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        let editor = Rc::clone(me);
        keys.connect_key_pressed(move |_, keyval, _, modifiers| {
            if !modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            if keyval == gdk::Key::z && !modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                editor.undo();
                glib::Propagation::Stop
            } else if keyval == gdk::Key::y
                || (keyval == gdk::Key::z && modifiers.contains(gdk::ModifierType::SHIFT_MASK))
            {
                editor.redo();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        me.view.add_controller(keys);
    }

    /// One observer for every buffer mutation.
    ///
    /// It does two jobs: feeds auto-completion after `.`/`->` and drives the
    /// undo batch sampler. Buffer changes are the ground truth for text
    /// edits, unlike keystrokes, which some input stacks deliver as commits.
    fn install_change_hub(me: &Rc<Self>) {
        // --- completion auto-popup --------------------------------------
        let view = me.view.clone();
        let popover = me.completion.clone();
        let list = me.completion_list.clone();
        let candidates = Rc::clone(&me.candidates);
        let index = Rc::clone(&me.index);
        me.buffer.connect_changed(move |_| {
            let view = view.clone();
            let popover = popover.clone();
            let list = list.clone();
            let candidates = Rc::clone(&candidates);
            let index = Rc::clone(&index);
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

        // --- undo batching ----------------------------------------------
        // A batch starts at the first change after the previous one settled
        // (500 ms of quiet) and stores the state from before the batch.
        let last_sample = Rc::clone(&me.last_sample);
        let batch_open = Rc::clone(&me.batch_open);
        let editor_for_batch = Rc::clone(me);
        let buffer_for_batch = me.buffer.clone();
        me.buffer.connect_changed(move |_| {
            editor_for_batch.modified.replace(true);
            if !batch_open.get() {
                let previous = last_sample.borrow().clone();
                editor_for_batch.push_undo_snapshot(&previous);
                batch_open.set(true);
                let sample = Rc::clone(&last_sample);
                let open = Rc::clone(&batch_open);
                let buffer = buffer_for_batch.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    sample.replace(buffer_text(&buffer));
                    open.set(false);
                });
            }
        });
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
    }

    /// Shows the find bar; `with_replace` also reveals the replace row.
    pub fn show_find(&self, with_replace: bool) {
        self.find_bar.set_visible(true);
        self.replace_row.set_visible(with_replace);
        *self.search.borrow_mut() = None;
        self.find_entry.grab_focus();
    }

    /// Hides the find bar and drops the live search context.
    pub fn hide_find(&self) {
        self.find_bar.set_visible(false);
        self.take_search();
    }

    /// Whether the find bar is currently shown.
    #[must_use]
    pub fn find_bar_visible(&self) -> bool {
        self.find_bar.is_visible()
    }

    /// The text to look for, or `None` while the entry is empty.
    fn find_text(&self) -> Option<String> {
        let text = self.find_entry.text().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Selects the next match after the cursor.
    pub fn find_next(&self) {
        if self.find_text().is_none() {
            self.find_info.set_text("Find: empty");
            return;
        }
        let search = self.ensure_search();
        let Some((start, end, _)) = search
            .as_ref()
            .and_then(|s| s.forward(&self.buffer.iter_at_mark(&self.buffer.get_insert())))
        else {
            self.find_info.set_text("No matches");
            return;
        };
        self.select_match(&start, &end);
    }

    /// Selects the previous match before the cursor.
    pub fn find_prev(&self) {
        if self.find_text().is_none() {
            self.find_info.set_text("Find: empty");
            return;
        }
        let search = self.ensure_search();
        let Some((start, end, _)) = search
            .as_ref()
            .and_then(|s| s.backward(&self.buffer.iter_at_mark(&self.buffer.get_insert())))
        else {
            self.find_info.set_text("No matches");
            return;
        };
        self.select_match(&start, &end);
    }

    /// Replaces the selected match, if the selection is a match.
    pub fn replace_current(&self) {
        let search = self.ensure_search();
        let Some(search) = search.as_ref() else {
            return;
        };
        let Some((mut start, mut end)) = self.buffer.selection_bounds() else {
            return;
        };
        if start == end {
            return;
        }
        let replacement = self.replace_entry.text().to_string();
        if search.replace(&mut start, &mut end, &replacement).is_err() {
            self.find_info.set_text("Replace failed");
            return;
        }
        self.find_next();
    }

    /// Replaces every match in the buffer, reporting the count.
    pub fn replace_all(&self) {
        if self.find_text().is_none() {
            self.find_info.set_text("Find: empty");
            return;
        }
        let search = self.ensure_search();
        let Some(search) = search.as_ref() else {
            return;
        };
        let replacement = self.replace_entry.text().to_string();
        let mut count = 0usize;
        let mut cursor = self.buffer.start_iter();
        while let Some((mut start, mut end, _)) = search.forward(&cursor) {
            if search.replace(&mut start, &mut end, &replacement).is_err() {
                self.find_info.set_text("Replace failed");
                return;
            }
            count += 1;
            cursor = self
                .buffer
                .iter_at_offset(start.offset() + replacement.chars().count() as i32);
        }
        self.find_info
            .set_text(&format!("Replaced {count} match(es)"));
    }

    /// Selects and scrolls to a match.
    fn select_match(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.buffer.select_range(start, end);
        let mut scroll_target = *start;
        self.view
            .scroll_to_iter(&mut scroll_target, 0.25, false, 0.0, 0.0);
        let line = start.line() + 1;
        self.find_info.set_text(&format!("Line {line}"));
    }

    /// Creates (once) the search context mirroring the find entry text.
    fn ensure_search(&self) -> std::cell::RefMut<'_, Option<sourceview::SearchContext>> {
        if self.search.borrow().is_none() {
            let settings = sourceview::SearchSettings::builder()
                .search_text(self.find_entry.text().to_string())
                .case_sensitive(false)
                .wrap_around(true)
                .regex_enabled(false)
                .build();
            let context = sourceview::SearchContext::new(&self.buffer, Some(&settings));
            context.set_highlight(true);
            *self.search.borrow_mut() = Some(context);
        }
        self.search.borrow_mut()
    }

    /// Drops the search context (highlight disappears).
    fn take_search(&self) {
        *self.search.borrow_mut() = None;
    }

    /// Wires the find bar buttons and its keyboard behaviour. Called with the
    /// editor's own `Rc`, so the callbacks can call editor methods.
    pub fn install_find_bar(
        me: &Rc<Self>,
        prev: &gtk::Button,
        next: &gtk::Button,
        replace: &gtk::Button,
        replace_all: &gtk::Button,
        close: &gtk::Button,
    ) {
        // Escape in either entry closes the bar. Each entry needs its own
        // controller: a glib clone shares the underlying object and GTK
        // rejects adding one controller to two widgets.
        let weak_bar = me.find_bar.downgrade();
        let escape_bar = weak_bar.clone();
        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                if let Some(bar) = escape_bar.upgrade() {
                    bar.set_visible(false);
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        me.find_entry.add_controller(escape);
        let escape_bar = weak_bar;
        let escape = gtk::EventControllerKey::new();
        escape.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                if let Some(bar) = escape_bar.upgrade() {
                    bar.set_visible(false);
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        me.replace_entry.add_controller(escape);

        // Buttons stay out of the tab chain so Tab moves entry -> entry.
        prev.set_focusable(false);
        next.set_focusable(false);
        close.set_focusable(false);
        replace.set_focusable(false);
        replace_all.set_focusable(false);

        // Shift+Enter in the find entry jumps to the previous match.
        let shift_prev = Rc::clone(me);
        let shift_keys = gtk::EventControllerKey::new();
        shift_keys.connect_key_pressed(move |_, keyval, _, modifiers| {
            if keyval == gdk::Key::Return && modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                shift_prev.find_prev();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        me.find_entry.add_controller(shift_keys);

        // Enter in the find entry jumps to the next match; in the replace
        // entry it replaces the current one, like Dev-C++.
        let enter_next = Rc::clone(me);
        me.find_entry
            .connect_activate(move |_| enter_next.find_next());
        let enter_replace = Rc::clone(me);
        me.replace_entry
            .connect_activate(move |_| enter_replace.replace_current());

        // Editing the search text invalidates the live context.
        let invalidate = Rc::clone(me);
        me.find_entry.connect_changed(move |_| {
            if invalidate.search.borrow().is_some() {
                invalidate.search.borrow_mut().take();
            }
        });

        let prev_editor = Rc::clone(me);
        prev.connect_clicked(move |_| prev_editor.find_prev());
        let next_editor = Rc::clone(me);
        next.connect_clicked(move |_| next_editor.find_next());
        let replace_editor = Rc::clone(me);
        replace.connect_clicked(move |_| replace_editor.replace_current());
        let all_editor = Rc::clone(me);
        replace_all.connect_clicked(move |_| all_editor.replace_all());
        let bar = me.find_bar.clone();
        close.connect_clicked(move |_| bar.set_visible(false));
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

/// Maximum undo snapshots kept per editor.
const UNDO_LIMIT: usize = 512;

/// The full text of `buffer`.
fn buffer_text(buffer: &sourceview::Buffer) -> String {
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}
