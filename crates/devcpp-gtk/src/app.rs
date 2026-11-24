//! The main application window.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use devcpp_core::build::{self, BuildJob, BuildMessage, RunMode};
use devcpp_core::config::Config;
use devcpp_core::debugger::Debugger;
use devcpp_core::project::Project;
use devcpp_core::symbols::SymbolIndex;
use devcpp_core::templates;
use devcpp_core::toolchain::Toolchain;

use crate::editor::Editor;
use crate::panels::{self, ClassPanel, DebugPanel, OutputPanel, ProjectPanel};

/// Shared application state.
struct AppState {
    /// The currently open project.
    project: Option<Project>,
    /// Open editors, in tab order.
    editors: Vec<Rc<Editor>>,
    /// The project symbol index.
    index: Rc<RefCell<SymbolIndex>>,
    /// The running build job.
    build_job: Option<BuildJob>,
    /// The active GDB session.
    debugger: Option<Debugger>,
    /// User configuration.
    config: Config,
    /// The detected toolchain.
    toolchain: Toolchain,
}

/// Starts the GTK application.
///
/// # Errors
/// Returns an error when GTK cannot be initialized.
pub fn run() -> anyhow::Result<()> {
    let app = gtk::Application::builder()
        .application_id("org.devcpp.ide")
        // Accept "devcpp-gtk file.dev" and "devcpp-gtk main.cpp", like
        // Dev-C++ when a project or unit is associated with the IDE.
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_activate(build_ui);
    app.connect_open(open_files_from_launch);
    app.run();
    Ok(())
}

/// The UI of a running window plus its shared state.
type LiveUi = (Rc<Ui>, Rc<RefCell<AppState>>);

thread_local! {
    /// The live UI of the running window, set when it is built. File
    /// arguments are handled by the `open` signal, which fires after
    /// `activate`, so the window already exists here.
    static LIVE_UI: RefCell<Option<LiveUi>> = const { RefCell::new(None) };
}

/// Builds the UI for files passed on the command line.
fn open_files_from_launch(app: &gtk::Application, files: &[gio::File], _hint: &str) {
    if app.active_window().is_none() {
        build_ui(app);
    }
    LIVE_UI.with(|slot| {
        let Some((ui, state)) = slot
            .borrow()
            .as_ref()
            .map(|(u, s)| (Rc::clone(u), Rc::clone(s)))
        else {
            return;
        };
        for file in files {
            let Some(path) = file.path() else {
                continue;
            };
            open_launch_target(&ui, &state, &path);
        }
    });
}

/// Opens a launch argument: `.dev` loads the project (and its units), any
/// other file opens as a source tab.
fn open_launch_target(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>, path: &Path) {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dev"))
    {
        match Project::load(path) {
            Ok(project) => {
                ui.project_panel.set_project(&project);
                let symbol_index = index_for(&project);
                ui.class_panel.set_index(&symbol_index);
                *state.borrow_mut().index.borrow_mut() = symbol_index;
                state.borrow_mut().project = Some(project);
                panels::set_status(&ui.statusbar, "Project opened");
                open_project_sources(ui, state);
            }
            Err(e) => panels::show_error(&ui.window, "Cannot open project", &e.to_string()),
        }
    } else {
        open_file(ui, state, path);
    }
}

fn build_ui(app: &gtk::Application) {
    let config = Config::load();
    let toolchain = Toolchain::detect();

    let state = Rc::new(RefCell::new(AppState {
        project: None,
        editors: Vec::new(),
        index: Rc::new(RefCell::new(SymbolIndex::default())),
        build_job: None,
        debugger: None,
        config,
        toolchain,
    }));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("devcpp — Dev-C++ for Linux")
        .default_width(1200)
        .default_height(760)
        // An IDE opens maximized, like Dev-C++ on Windows.
        .maximized(true)
        .build();

    let notebook = gtk::Notebook::builder().hexpand(true).vexpand(true).build();
    notebook.set_tab_pos(gtk::PositionType::Top);

    let project_panel = Rc::new(ProjectPanel::new());
    let class_panel = Rc::new(ClassPanel::new());
    let output_panel = Rc::new(OutputPanel::new());
    let debug_panel = Rc::new(DebugPanel::new());

    // Left side: project tree over class browser.
    let side_notebook = gtk::Notebook::builder().vexpand(true).build();
    side_notebook.append_page(
        &project_panel.widget,
        Some(&gtk::Label::new(Some("Project"))),
    );
    side_notebook.append_page(&class_panel.widget, Some(&gtk::Label::new(Some("Classes"))));
    let side_scroller = gtk::ScrolledWindow::builder()
        .hexpand(false)
        .vexpand(true)
        .child(&side_notebook)
        .build();

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .build();
    paned.set_start_child(Some(&side_scroller));
    paned.set_end_child(Some(&notebook));
    paned.set_position(260);

    // Bottom: compiler output over debug panel.
    let bottom_notebook = gtk::Notebook::builder().hexpand(true).build();
    bottom_notebook.append_page(
        &output_panel.widget,
        Some(&gtk::Label::new(Some("Compiler Output"))),
    );
    bottom_notebook.append_page(&debug_panel.widget, Some(&gtk::Label::new(Some("Debug"))));
    bottom_notebook.set_size_request(-1, 220);

    let main_paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    main_paned.set_start_child(Some(&paned));
    main_paned.set_end_child(Some(&bottom_notebook));
    // A fixed pixel position collapses the output panel on small windows, so
    // the split is derived from the real height once it is known.
    let paned_for_layout = main_paned.clone();
    glib::idle_add_local_once(move || {
        let height = paned_for_layout.height();
        if height > 120 {
            paned_for_layout.set_position(((height as f32) * 0.72) as i32);
        }
    });

    let statusbar = gtk::Label::new(Some("Ready"));
    statusbar.set_xalign(0.0);
    statusbar.set_margin_start(6);
    statusbar.set_margin_end(6);
    let toolbar = build_toolbar();
    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    vbox.append(&toolbar);
    vbox.append(&main_paned);
    vbox.append(&statusbar);
    window.set_child(Some(&vbox));

    let ui = Rc::new(Ui {
        window,
        notebook,
        project_panel,
        class_panel,
        output_panel,
        debug_panel,
        statusbar,
    });

    install_actions(app, &ui, &state);
    install_menu(&ui, &state);
    install_mouse_gestures(&ui, &state);
    install_project_tree(&ui, &state);

    panels::set_status(&ui.statusbar, "Ready");
    // Start with one editor tab focused, like any editor-first IDE: typing
    // must never land on a toolbar button.
    open_new_editor(&ui, &state);
    install_window_keys(&ui, &state);
    // Quitting must not leave the UI slot dangling: it holds the only strong
    // references outside the window tree.
    ui.window.connect_close_request(|_| {
        LIVE_UI.with(|slot| *slot.borrow_mut() = None);
        glib::Propagation::Proceed
    });
    LIVE_UI.with(|slot| {
        *slot.borrow_mut() = Some((Rc::clone(&ui), Rc::clone(&state)));
    });
    ui.window.present();
}

/// Window-level keys: Escape closes an open find/replace bar first.
fn install_window_keys(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let keys = gtk::EventControllerKey::new();
    let ui_keys = Rc::clone(ui);
    let state_keys = state.clone();
    keys.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            if let Some(editor) = current_editor(&ui_keys, &state_keys) {
                if editor.find_bar_visible() {
                    editor.hide_find();
                    editor.view.grab_focus();
                    return gtk::glib::Propagation::Stop;
                }
            }
        }
        gtk::glib::Propagation::Proceed
    });
    ui.window.add_controller(keys);
}

/// The widgets the actions operate on.
struct Ui {
    window: gtk::ApplicationWindow,
    notebook: gtk::Notebook,
    project_panel: Rc<ProjectPanel>,
    class_panel: Rc<ClassPanel>,
    output_panel: Rc<OutputPanel>,
    debug_panel: Rc<DebugPanel>,
    statusbar: gtk::Label,
}

/// Global accelerators, in GTK4 `<Control>f9` syntax.
const ACCELERATORS: &[(&str, &str)] = &[
    ("build", "<Control>F9"),
    ("run", "<Control>F10"),
    ("debug", "<Control>F8"),
    ("new-file", "<Control>n"),
    ("open", "<Control>o"),
    ("save", "<Control>s"),
    ("save-as", "<Shift><Control>s"),
    ("quit", "<Control>q"),
    ("complete", "<Control>space"),
    ("toggle-breakpoint", "F2"),
    ("find", "<Control>f"),
    ("replace", "<Control>h"),
    ("cut", "<Control>x"),
    ("copy", "<Control>c"),
    ("paste", "<Control>v"),
    ("select-all", "<Control>a"),
];

fn install_actions(app: &gtk::Application, ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    // Accelerators are registered on the application, which routes "win."
    // actions to the active window. This replaces per-key shortcut
    // controllers and is the standard GTK4 mechanism.
    for (action, accelerator) in ACCELERATORS {
        app.set_accels_for_action(&format!("win.{action}"), &[accelerator]);
    }

    let group = gio::SimpleActionGroup::new();

    let add = |name: &str, handler: Box<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| handler());
        group.add_action(&action);
    };

    let ui_build = ui.clone();
    let state_build = state.clone();
    add(
        "build",
        Box::new(move || build_project_now(&ui_build, &state_build)),
    );

    let ui_run = ui.clone();
    let state_run = state.clone();
    add(
        "run",
        Box::new(move || run_project_now(&ui_run, &state_run)),
    );

    let ui_debug = ui.clone();
    let state_debug = state.clone();
    add(
        "debug",
        Box::new(move || debug_project_now(&ui_debug, &state_debug)),
    );

    let ui_new = ui.clone();
    let state_new = state.clone();
    add(
        "new-file",
        Box::new(move || action_new_file_now(&ui_new, &state_new)),
    );

    let ui_newproj = ui.clone();
    let state_newproj = state.clone();
    add(
        "new-project",
        Box::new(move || show_new_project_wizard(&ui_newproj, &state_newproj)),
    );

    let ui_open = ui.clone();
    let state_open = state.clone();
    add(
        "open",
        Box::new(move || action_open_now(&ui_open, &state_open)),
    );

    let ui_save = ui.clone();
    let state_save = state.clone();
    add(
        "save",
        Box::new(move || action_save_now(&ui_save, &state_save)),
    );

    let ui_saveas = ui.clone();
    let state_saveas = state.clone();
    add(
        "save-as",
        Box::new(move || action_save_as_now(&ui_saveas, &state_saveas)),
    );

    let ui_quit = ui.clone();
    add("quit", Box::new(move || ui_quit.window.close()));

    let ui_complete = ui.clone();
    let state_complete = state.clone();
    add(
        "complete",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_complete, &state_complete) {
                editor.show_completion();
            }
        }),
    );

    let ui_break = ui.clone();
    let state_break = state.clone();
    add(
        "toggle-breakpoint",
        Box::new(move || toggle_breakpoint(&ui_break, &state_break)),
    );

    let ui_find = ui.clone();
    let state_find = state.clone();
    add(
        "find",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_find, &state_find) {
                editor.show_find(false);
            }
        }),
    );

    let ui_replace = ui.clone();
    let state_replace = state.clone();
    add(
        "replace",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_replace, &state_replace) {
                editor.show_find(true);
            }
        }),
    );

    let ui_cut = ui.clone();
    let state_cut = state.clone();
    add(
        "cut",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_cut, &state_cut) {
                editor.snapshot_undo();
                let clipboard = editor.view.clipboard();
                editor.buffer.cut_clipboard(&clipboard, true);
            }
        }),
    );

    let ui_copy = ui.clone();
    let state_copy = state.clone();
    add(
        "copy",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_copy, &state_copy) {
                let clipboard = editor.view.clipboard();
                editor.buffer.copy_clipboard(&clipboard);
            }
        }),
    );

    let ui_paste = ui.clone();
    let state_paste = state.clone();
    add(
        "paste",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_paste, &state_paste) {
                editor.snapshot_undo();
                let clipboard = editor.view.clipboard();
                editor.buffer.paste_clipboard(&clipboard, None, true);
            }
        }),
    );

    let ui_select = ui.clone();
    let state_select = state.clone();
    add(
        "select-all",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_select, &state_select) {
                let start = editor.buffer.start_iter();
                let end = editor.buffer.end_iter();
                editor.buffer.select_range(&start, &end);
            }
        }),
    );

    let ui_undo = ui.clone();
    let state_undo = state.clone();
    add(
        "undo",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_undo, &state_undo) {
                editor.undo();
            }
        }),
    );

    let ui_redo = ui.clone();
    let state_redo = state.clone();
    add(
        "redo",
        Box::new(move || {
            if let Some(editor) = current_editor(&ui_redo, &state_redo) {
                editor.redo();
            }
        }),
    );

    let ui_about = ui.clone();
    add(
        "about",
        Box::new(move || {
            panels::show_info(
                &ui_about.window,
                "About devcpp",
                "devcpp — a native Linux C/C++ IDE written in Rust.\n\
             A faithful clone of Embarcadero Dev-C++ 6.x.\n\n\
             Version 1.0.0",
            );
        }),
    );

    let ui_tools = ui.clone();
    let state_tools = state.clone();
    add(
        "tools",
        Box::new(move || show_tools(&ui_tools, &state_tools)),
    );

    let ui_goto = ui.clone();
    let state_goto = state.clone();
    add(
        "goto-error",
        Box::new(move || goto_first_error(&ui_goto, &state_goto)),
    );

    let state_run_dbg = state.clone();
    add(
        "debug-run",
        Box::new(move || {
            if let Some(debugger) = state_run_dbg.borrow().debugger.as_ref() {
                let _ = debugger.run();
            }
        }),
    );

    let state_cont = state.clone();
    add(
        "debug-continue",
        Box::new(move || {
            if let Some(debugger) = state_cont.borrow().debugger.as_ref() {
                let _ = debugger.cont();
            }
        }),
    );

    let state_step = state.clone();
    add(
        "debug-step",
        Box::new(move || {
            if let Some(debugger) = state_step.borrow().debugger.as_ref() {
                let _ = debugger.step();
            }
        }),
    );

    let state_next = state.clone();
    add(
        "debug-next",
        Box::new(move || {
            if let Some(debugger) = state_next.borrow().debugger.as_ref() {
                let _ = debugger.next();
            }
        }),
    );

    ui.window.insert_action_group("win", Some(&group));
}

fn install_menu(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let menu = gio::Menu::new();

    let file_menu = gio::Menu::new();
    file_menu.append(Some("_New Project…"), Some("win.new-project"));
    file_menu.append(Some("_New File"), Some("win.new-file"));
    file_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    file_menu.append(Some("_Open…"), Some("win.open"));
    file_menu.append(Some("_Save"), Some("win.save"));
    file_menu.append(Some("Save _As…"), Some("win.save-as"));
    file_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    file_menu.append(Some("_Quit"), Some("win.quit"));
    menu.append_submenu(Some("_File"), &file_menu);

    let edit_menu = gio::Menu::new();
    edit_menu.append(Some("_Undo"), Some("win.undo"));
    edit_menu.append(Some("_Redo"), Some("win.redo"));
    edit_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    edit_menu.append(Some("Cu_t"), Some("win.cut"));
    edit_menu.append(Some("_Copy"), Some("win.copy"));
    edit_menu.append(Some("_Paste"), Some("win.paste"));
    edit_menu.append(Some("Select _All"), Some("win.select-all"));
    edit_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    edit_menu.append(Some("_Find…"), Some("win.find"));
    edit_menu.append(Some("_Replace…"), Some("win.replace"));
    edit_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    edit_menu.append(Some("Complete _Code"), Some("win.complete"));
    edit_menu.append(Some("Toggle _Breakpoint"), Some("win.toggle-breakpoint"));
    menu.append_submenu(Some("_Edit"), &edit_menu);

    let build_menu = gio::Menu::new();
    build_menu.append(Some("_Compile"), Some("win.build"));
    build_menu.append(Some("Compile and _Run"), Some("win.run"));
    build_menu.append(Some("Jump to _Error"), Some("win.goto-error"));
    build_menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    build_menu.append(Some("_Debug"), Some("win.debug"));
    menu.append_submenu(Some("_Build"), &build_menu);

    let debug_menu = gio::Menu::new();
    debug_menu.append(Some("_Run"), Some("win.debug-run"));
    debug_menu.append(Some("_Continue"), Some("win.debug-continue"));
    debug_menu.append(Some("_Step"), Some("win.debug-step"));
    debug_menu.append(Some("Ne_xt"), Some("win.debug-next"));
    menu.append_submenu(Some("De_bug"), &debug_menu);

    let tools_menu = gio::Menu::new();
    tools_menu.append(Some("_Toolchain…"), Some("win.tools"));
    menu.append_submenu(Some("_Tools"), &tools_menu);

    let help_menu = gio::Menu::new();
    help_menu.append(Some("_About"), Some("win.about"));
    menu.append_submenu(Some("_Help"), &help_menu);

    // GTK4 has no menu model on windows: the menu bar lives in the header bar.
    let menu_bar = gtk::PopoverMenuBar::from_model(Some(&menu));
    let header = gtk::HeaderBar::builder()
        .title_widget(&gtk::Label::new(Some("devcpp")))
        .build();
    header.pack_start(&menu_bar);
    ui.window.set_titlebar(Some(&header));

    let _ = state;
}

fn build_toolbar() -> gtk::Box {
    let toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .margin_top(4)
        .margin_bottom(4)
        .margin_start(4)
        .margin_end(4)
        .build();

    let button = |icon: &str, tooltip: &str, action: &str| {
        let button = gtk::Button::from_icon_name(icon);
        button.set_tooltip_text(Some(tooltip));
        button.set_action_name(Some(action));
        button
    };

    toolbar.append(&button("document-new", "New project", "win.new-project"));
    toolbar.append(&button("document-open", "Open project", "win.open"));
    toolbar.append(&button("document-save", "Save", "win.save"));
    toolbar.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    toolbar.append(&button("media-record", "Compile", "win.build"));
    toolbar.append(&button("system-run", "Compile and run", "win.run"));
    toolbar.append(&button("debug-run", "Debug", "win.debug"));
    toolbar
}

/// Opens the file that the user double-clicks in the project tree.
fn install_project_tree(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let ui_tree = Rc::clone(ui);
    let state_tree = Rc::clone(state);
    ui.project_panel
        .view
        .connect_row_activated(move |_, path, _column| {
            let Some(name) = panels::row_path(&ui_tree.project_panel.view, path) else {
                return;
            };
            let base = state_tree
                .borrow()
                .project
                .as_ref()
                .map_or_else(|| PathBuf::from("."), |p| p.directory());
            let candidate = if name.is_absolute() {
                name
            } else {
                base.join(name)
            };
            if candidate.is_file() {
                open_file(&ui_tree, &state_tree, &candidate);
            }
        });
}

fn install_mouse_gestures(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    // Compiler output: click a diagnostic to jump to it, like Dev-C++.
    let ui_output = Rc::clone(ui);
    let state_output = Rc::clone(state);
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, x, y| {
        let panel = &ui_output.output_panel;
        let (bx, by) =
            panel
                .view
                .window_to_buffer_coords(gtk::TextWindowType::Text, x as i32, y as i32);
        if let Some(iter) = panel.view.iter_at_location(bx, by) {
            if let Some(diagnostic) = panel.diagnostic_at_line(iter.line()) {
                jump_to_diagnostic(&ui_output, &state_output, &diagnostic);
            }
        }
    });
    ui.output_panel.view.add_controller(click);

    let ui_click = Rc::clone(ui);
    let controller = gtk::GestureClick::new();
    controller.set_button(gdk::BUTTON_SECONDARY);
    controller.connect_pressed(move |_, _, x, y| {
        show_context_menu(&ui_click, x, y);
    });
    ui.window.add_controller(controller);
}

fn current_editor(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) -> Option<Rc<Editor>> {
    let page = ui.notebook.current_page()?;
    let index = page as usize;
    let app_state = state.borrow();
    app_state.editors.get(index).cloned()
}

/// Opens `path` in the editor, reusing an already open tab.
fn open_file(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>, path: &Path) {
    let index = Rc::clone(&state.borrow().index);
    let editor = Editor::new(path, index);
    editor.apply_language();
    let (family, size) = {
        let cfg = &state.borrow().config;
        (cfg.editor.font_family.clone(), cfg.editor.font_size)
    };
    editor.apply_font(&family, size);
    // A file already open in another tab is focused instead of duplicated.
    let existing = {
        let app_state = state.borrow();
        app_state
            .editors
            .iter()
            .position(|e| *e.path.borrow() == path)
    };
    if let Some(index) = existing {
        ui.notebook.set_current_page(Some(index as u32));
        state.borrow().editors[index].view.grab_focus();
        return;
    }
    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".to_string());
    let page = ui
        .notebook
        .append_page(&editor.widget, Some(&gtk::Label::new(Some(&title))));
    ui.notebook.set_current_page(Some(page));
    state.borrow_mut().editors.push(editor);
    if let Some(editor) = current_editor(ui, state) {
        editor.view.grab_focus();
    }
}

/// Opens a new in-memory editor tab, like Dev-C++'s "New Source" before the
/// first save. The path is empty until Save As picks one.
fn open_new_editor(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let index = Rc::clone(&state.borrow().index);
    let editor = Editor::new(Path::new(""), index);
    let (family, size) = {
        let cfg = &state.borrow().config;
        (cfg.editor.font_family.clone(), cfg.editor.font_size)
    };
    editor.apply_font(&family, size);
    let page = ui
        .notebook
        .append_page(&editor.widget, Some(&gtk::Label::new(Some("untitled"))));
    ui.notebook.set_current_page(Some(page));
    state.borrow_mut().editors.push(editor);
    editor_focus(ui, state);
}

/// Moves keyboard focus into the current editor, so toolbar buttons never
/// swallow typing (e.g. Space activating a button).
fn editor_focus(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    if let Some(editor) = current_editor(ui, state) {
        editor.view.grab_focus();
    }
}

fn jump_to_diagnostic(
    ui: &Rc<Ui>,
    state: &Rc<RefCell<AppState>>,
    diag: &devcpp_core::diagnostics::Diagnostic,
) {
    let Some(file) = diag.file.clone() else {
        return;
    };
    let line = diag.line.unwrap_or(1);
    let path = if file.is_absolute() {
        file.clone()
    } else {
        let base = state
            .borrow()
            .project
            .as_ref()
            .map_or_else(|| PathBuf::from("."), |p| p.directory());
        base.join(&file)
    };
    // Find an already open editor for this file.
    let existing = {
        let app_state = state.borrow();
        app_state
            .editors
            .iter()
            .position(|e| *e.path.borrow() == path)
    };
    if let Some(index) = existing {
        let editor = state.borrow().editors[index].clone();
        editor.goto_line(line);
        ui.notebook.set_current_page(Some(index as u32));
        panels::set_status(
            &ui.statusbar,
            &format!("Jumped to {}:{line}", file.display()),
        );
        return;
    }
    open_file(ui, state, &path);
    if let Some(editor) = current_editor(ui, state) {
        editor.goto_line(line);
    }
    panels::set_status(
        &ui.statusbar,
        &format!("Jumped to {}:{line}", file.display()),
    );
}

/// Jumps to the first error of the last build, like Dev-C++'s "jump to next
/// error" button.
fn goto_first_error(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let Some(diag) = ui.output_panel.diagnostic(0) else {
        panels::set_status(&ui.statusbar, "No errors to jump to");
        return;
    };
    jump_to_diagnostic(ui, state, &diag);
}

/// Starts the debugger session and polls its events.
fn start_debugger(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) -> Result<(), String> {
    let Some(project) = state.borrow().project.clone() else {
        return Err("Open or create a project first.".to_string());
    };
    let gdb = state.borrow().toolchain.debugger.command.clone();
    let debugger = Debugger::start(&gdb).map_err(|e| e.to_string())?;
    let exe = project.executable_path();
    debugger.load_program(&exe).map_err(|e| e.to_string())?;
    ui.debug_panel.append(&format!("Loaded {}", exe.display()));
    state.borrow_mut().debugger = Some(debugger);
    Ok(())
}

/// Polls the debugger and refreshes the debug panels.
fn poll_debugger(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let ui_clone = Rc::clone(ui);
    let state_clone = Rc::clone(state);
    glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
        let events: Vec<_> = {
            let app_state = state_clone.borrow();
            app_state
                .debugger
                .as_ref()
                .map(|d| d.try_recv().into_iter().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        for event in &events {
            ui_clone.debug_panel.handle_event(event);
        }
        glib::ControlFlow::Continue
    });
}

fn build_project_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let Some(project) = state.borrow().project.clone() else {
        panels::show_error(&ui.window, "No project", "Open or create a project first.");
        return;
    };
    ui.output_panel.clear();
    panels::set_status(&ui.statusbar, "Compiling…");
    let job = match build::start(&project, RunMode::BuildOnly) {
        Ok(job) => job,
        Err(e) => {
            panels::show_error(&ui.window, "Build failed", &e.to_string());
            return;
        }
    };
    state.borrow_mut().build_job = Some(job);
    poll_build(ui, state);
}

fn poll_build(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let ui_clone = Rc::clone(ui);
    let state_clone = Rc::clone(state);
    glib::timeout_add_local(std::time::Duration::from_millis(60), move || {
        let job = state_clone.borrow_mut().build_job.take();
        let Some(job) = job else {
            return glib::ControlFlow::Break;
        };
        let mut finished = false;
        let mut success = false;
        while let Some(message) = job.try_recv() {
            match message {
                BuildMessage::Line(line) => ui_clone.output_panel.append(&line),
                BuildMessage::Diagnostic(diag) => ui_clone.output_panel.add_diagnostic(&diag),
                BuildMessage::Error(e) => ui_clone.output_panel.append(&format!("error: {e}")),
                BuildMessage::Finished(ok) => {
                    success = ok;
                    finished = true;
                }
                BuildMessage::Phase(phase) => match phase {
                    build::BuildPhase::Preparing => {}
                    build::BuildPhase::Compiling => {
                        panels::set_status(&ui_clone.statusbar, "Compiling…");
                    }
                    build::BuildPhase::Running => {
                        panels::set_status(&ui_clone.statusbar, "Running…");
                    }
                    build::BuildPhase::Finished => {}
                },
            }
        }
        if finished {
            panels::set_status(
                &ui_clone.statusbar,
                if success {
                    "Build succeeded"
                } else {
                    "Build failed"
                },
            );
            return glib::ControlFlow::Break;
        }
        state_clone.borrow_mut().build_job = Some(job);
        glib::ControlFlow::Continue
    });
}

fn run_project_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let Some(project) = state.borrow().project.clone() else {
        panels::show_error(&ui.window, "No project", "Open or create a project first.");
        return;
    };
    ui.output_panel.clear();
    panels::set_status(&ui.statusbar, "Compiling…");
    let job = match build::start(&project, RunMode::BuildAndRun) {
        Ok(job) => job,
        Err(e) => {
            panels::show_error(&ui.window, "Build failed", &e.to_string());
            return;
        }
    };
    state.borrow_mut().build_job = Some(job);
    poll_build(ui, state);
}

fn debug_project_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    match start_debugger(ui, state) {
        Ok(()) => {
            panels::set_status(&ui.statusbar, "Debugger ready");
            poll_debugger(ui, state);
        }
        Err(message) => panels::show_error(&ui.window, "Cannot start the debugger", &message),
    }
}

fn toggle_breakpoint(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let Some(editor) = current_editor(ui, state) else {
        return;
    };
    let line = editor.cursor_line();
    let path = editor.path.borrow().clone();
    let app_state = state.borrow_mut();
    let Some(debugger) = app_state.debugger.as_ref() else {
        panels::set_status(&ui.statusbar, "Start the debugger first");
        return;
    };
    match debugger.set_breakpoint(&path, line) {
        Ok(number) => {
            ui.debug_panel
                .append(&format!("Breakpoint {number} at {}:{line}", path.display()));
            panels::set_status(&ui.statusbar, &format!("Breakpoint {number} set"));
        }
        Err(e) => panels::show_error(&ui.window, "Cannot set breakpoint", &e.to_string()),
    }
}

/// Shows a folder chooser that fills `entry`, since GTK4 has no inline
/// folder button widget.
fn choose_folder_for(entry: &gtk::Entry, parent: &gtk::ApplicationWindow) {
    let dialog = gtk::FileChooserNative::builder()
        .title("Choose the project folder")
        .action(gtk::FileChooserAction::SelectFolder)
        .accept_label("Choose")
        .transient_for(parent)
        .modal(true)
        .build();
    let entry = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(folder) = dialog.file().and_then(|f| f.path()) {
                if let Ok(text) = folder.into_os_string().into_string() {
                    entry.set_text(&text);
                }
            }
        }
        dialog.destroy();
    });
    dialog.show();
}

/// The "New Project" wizard: name, template and target folder, like the
/// Dev-C++ "New Project" dialog.
fn show_new_project_wizard(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let dialog = gtk::Window::builder()
        .title("New Project")
        .modal(true)
        .transient_for(&ui.window)
        .default_width(460)
        .resizable(false)
        .build();

    let grid = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    // Project name.
    grid.append(&gtk::Label::new(Some("Project name")));
    let name_entry = gtk::Entry::builder()
        .activates_default(true)
        .placeholder_text("my-project")
        .build();
    grid.append(&name_entry);

    // Template picker with live description.
    grid.append(&gtk::Label::new(Some("Project type")));
    let template_choice = gtk::ComboBoxText::new();
    for template in templates::TEMPLATES {
        template_choice.append_text(template.name);
    }
    template_choice.set_active(Some(0));
    grid.append(&template_choice);

    let description = gtk::Label::new(Some(templates::TEMPLATES[0].description));
    description.set_xalign(0.0);
    description.set_wrap(true);
    grid.append(&description);

    let label_for_description = description.clone();
    template_choice.connect_changed(move |choice| {
        if let Some(index) = choice.active() {
            if let Some(template) = templates::TEMPLATES.get(index as usize) {
                label_for_description.set_text(template.description);
            }
        }
    });

    // Target folder: an entry filled by a native folder chooser.
    grid.append(&gtk::Label::new(Some("Location")));
    // Prefilled with ~/projects: the folder Dev-C++ suggests for new work, so
    // "Create" works with the first keystroke instead of an empty-field error.
    let default_location = std::env::var("HOME")
        .map(|home| Path::new(&home).join("projects"))
        .unwrap_or_else(|_| PathBuf::from("projects"));
    let folder_entry = gtk::Entry::builder()
        .placeholder_text("~/projects")
        .text(default_location.display().to_string())
        .build();
    grid.append(&folder_entry);
    let folder_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    folder_entry.set_hexpand(true);
    folder_row.append(&folder_entry);
    let browse = gtk::Button::with_label("Browse…");
    let entry_for_browse = folder_entry.clone();
    let window_for_browse = ui.window.clone();
    browse.connect_clicked(move |_| {
        choose_folder_for(&entry_for_browse, &window_for_browse);
    });
    folder_row.append(&browse);
    grid.append(&folder_row);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    let header = gtk::HeaderBar::builder().build();
    let cancel = gtk::Button::with_label("Cancel");
    let create = gtk::Button::with_label("Create");
    create.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&create);
    dialog.set_titlebar(Some(&header));
    content.append(&grid);
    dialog.set_child(Some(&content));

    // Escape closes the wizard, like any dialog.
    let dialog_escape = dialog.clone();
    let escape = gtk::EventControllerKey::new();
    escape.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            dialog_escape.close();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    dialog.add_controller(escape);

    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| dialog_cancel.close());
    let dialog_create = dialog.clone();
    let name_create = name_entry.clone();
    let choice_create = template_choice.clone();
    let folder_create = folder_entry.clone();
    let ui_create = Rc::clone(ui);
    let state_create = Rc::clone(state);
    let create_trigger = {
        let ui_create = Rc::clone(&ui_create);
        let state_create = Rc::clone(&state_create);
        let dialog_create = dialog_create.clone();
        let name_create = name_create.clone();
        let choice_create = choice_create.clone();
        let folder_create = folder_create.clone();
        move || {
            create_project_from_wizard(
                &ui_create,
                &state_create,
                &dialog_create,
                &name_create,
                &choice_create,
                &folder_create,
            );
        }
    };
    create.connect_clicked({
        let trigger = create_trigger.clone();
        move |_| trigger()
    });
    name_entry.connect_activate(move |_| create_trigger());

    dialog.present();
}

/// Validates the wizard fields, creates the project and opens it.
fn create_project_from_wizard(
    ui: &Rc<Ui>,
    state: &Rc<RefCell<AppState>>,
    dialog: &gtk::Window,
    name_entry: &gtk::Entry,
    template_choice: &gtk::ComboBoxText,
    folder_entry: &gtk::Entry,
) {
    let name = name_entry.text().trim().to_string();
    if name.is_empty() {
        panels::show_error(&ui.window, "Invalid name", "The project name is empty.");
        return;
    }
    if name.contains('/') || name.contains('\\') {
        panels::show_error(
            &ui.window,
            "Invalid name",
            "The project name may not contain path separators.",
        );
        return;
    }
    let folder_text = folder_entry.text().trim().to_string();
    if folder_text.is_empty() {
        panels::show_error(
            &ui.window,
            "No location",
            "Choose a folder for the project.",
        );
        return;
    }
    let folder = PathBuf::from(folder_text);
    let index = template_choice.active().unwrap_or(0) as usize;
    let Some(template) = templates::TEMPLATES.get(index) else {
        panels::show_error(&ui.window, "No template", "The chosen template is unknown.");
        return;
    };
    match templates::create(template, &name, &folder) {
        Ok(dev_path) => match Project::load(&dev_path) {
            Ok(project) => {
                ui.project_panel.set_project(&project);
                let symbol_index = index_for(&project);
                ui.class_panel.set_index(&symbol_index);
                *state.borrow_mut().index.borrow_mut() = symbol_index;
                state.borrow_mut().project = Some(project);
                panels::set_status(&ui.statusbar, "Project created");
                dialog.close();
                open_project_sources(ui, state);
            }
            Err(e) => {
                panels::show_error(&ui.window, "Cannot open project", &e.to_string());
            }
        },
        Err(e) => {
            panels::show_error(&ui.window, "Cannot create project", &e.to_string());
        }
    }
}

/// Closes the still-empty startup tabs once real files take over.
///
/// The window starts with one untitled tab so typing always lands in an
/// editor; without this, opening a project would leave a dead tab behind.
fn discard_scratch_tabs(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let scratch: Vec<usize> = {
        let app_state = state.borrow();
        app_state
            .editors
            .iter()
            .enumerate()
            .filter(|(_, editor)| {
                editor.path.borrow().as_os_str().is_empty()
                    && !*editor.modified.borrow()
                    && editor.text().is_empty()
            })
            .map(|(index, _)| index)
            .collect()
    };
    // Highest index first, so the remaining indices stay valid.
    for index in scratch.into_iter().rev() {
        state.borrow_mut().editors.remove(index);
        ui.notebook.remove_page(Some(index as u32));
    }
}

/// Opens every source unit of the current project, like Dev-C++ does after
/// creating a project.
fn open_project_sources(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let units: Vec<PathBuf> = match state.borrow().project.as_ref() {
        Some(project) => project
            .units
            .iter()
            .map(|u| u.absolute_path(&project.directory()))
            .collect(),
        None => return,
    };
    for path in units {
        open_file(ui, state, &path);
    }
    // The startup tab exists only to catch typing before a project is open;
    // real files take its place now.
    discard_scratch_tabs(ui, state);
}

fn action_new_file_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    {
        let dialog = gtk::FileChooserNative::builder()
            .title("New file")
            .action(gtk::FileChooserAction::Save)
            .accept_label("Create")
            .transient_for(&ui.window)
            .modal(true)
            .build();
        let ui = Rc::clone(ui);
        let state = Rc::clone(state);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|f| f.path()) {
                    if !path.exists() {
                        if let Some(parent) = path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::write(&path, "") {
                            panels::show_error(&ui.window, "Cannot create file", &e.to_string());
                            dialog.destroy();
                            return;
                        }
                    }
                    open_file(&ui, &state, &path);
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }
}

fn action_open_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    {
        let dialog = gtk::FileChooserNative::builder()
            .title("Open project or file")
            .action(gtk::FileChooserAction::Open)
            .transient_for(&ui.window)
            .modal(true)
            .build();
        let ui = Rc::clone(ui);
        let state = Rc::clone(state);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|f| f.path()) {
                    if path.extension().is_some_and(|e| e == "dev") {
                        match Project::load(&path) {
                            Ok(project) => {
                                ui.project_panel.set_project(&project);
                                let index = index_for(&project);
                                ui.class_panel.set_index(&index);
                                *state.borrow_mut().index.borrow_mut() = index;
                                state.borrow_mut().project = Some(project);
                                panels::set_status(&ui.statusbar, "Project opened");
                                open_project_sources(&ui, &state);
                            }
                            Err(e) => {
                                panels::show_error(
                                    &ui.window,
                                    "Cannot open project",
                                    &e.to_string(),
                                );
                            }
                        }
                    } else {
                        open_file(&ui, &state, &path);
                    }
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }
}

fn action_save_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    {
        if let Some(editor) = current_editor(ui, state) {
            // An unnamed file (the startup tab, or Ctrl+N) needs a name first:
            // Dev-C++ prompts instead of failing, and so does devcpp.
            if editor.path.borrow().as_os_str().is_empty() {
                action_save_as_now(ui, state);
                return;
            }
            if let Err(e) = editor.save() {
                panels::show_error(&ui.window, "Cannot save", &e.to_string());
            } else {
                panels::set_status(&ui.statusbar, "File saved");
            }
        }
        if let Some(project) = state.borrow().project.as_ref() {
            if let Err(e) = project.save() {
                panels::show_error(&ui.window, "Cannot save project", &e.to_string());
            }
        }
    }
}

fn action_save_as_now(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let Some(page) = ui.notebook.current_page() else {
        return;
    };
    let existing = {
        let app_state = state.borrow();
        app_state
            .editors
            .get(page as usize)
            .map(|e| e.path.borrow().clone())
    };
    let dialog = gtk::FileChooserNative::builder()
        .title("Save as")
        .action(gtk::FileChooserAction::Save)
        .transient_for(&ui.window)
        .modal(true)
        .build();
    if let Some(existing) = existing.filter(|p| !p.as_os_str().is_empty()) {
        let _ = dialog.set_file(&gio::File::for_path(&existing));
    }
    let ui = Rc::clone(ui);
    let state = Rc::clone(state);
    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(path) = dialog.file().and_then(|f| f.path()) {
                let editor = {
                    let app_state = state.borrow();
                    app_state.editors.get(page as usize).cloned()
                };
                if let Some(editor) = editor {
                    match editor.save_to(&path) {
                        Ok(()) => {
                            let title = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| "untitled".to_string());
                            ui.notebook.set_tab_label(
                                &editor.widget,
                                Some(&gtk::Label::new(Some(&title))),
                            );
                            editor.apply_language();
                            panels::set_status(&ui.statusbar, "File saved");
                        }
                        Err(e) => panels::show_error(&ui.window, "Cannot save", &e.to_string()),
                    }
                }
            }
        }
        dialog.destroy();
    });
    dialog.show();
}

fn index_for(project: &Project) -> SymbolIndex {
    let files: Vec<PathBuf> = project
        .units
        .iter()
        .map(|u| u.absolute_path(&project.directory()))
        .collect();
    SymbolIndex::build(&files)
}

fn show_tools(ui: &Rc<Ui>, state: &Rc<RefCell<AppState>>) {
    let tc = &state.borrow().toolchain;
    let mut text = String::new();
    text.push_str("C compiler:     ");
    text.push_str(&format!("{:?}\n", tc.c_compiler.path));
    text.push_str("C++ compiler:   ");
    text.push_str(&format!("{:?}\n", tc.cpp_compiler.path));
    text.push_str("Debugger:       ");
    text.push_str(&format!("{:?}\n", tc.debugger.path));
    text.push_str("Make:           ");
    text.push_str(&format!("{:?}\n", tc.make.path));
    text.push_str("Formatter:      ");
    text.push_str(&format!("{:?}\n", tc.astyle.path));
    text.push_str("Profiler:       ");
    text.push_str(&format!("{:?}\n", tc.gprof.path));
    panels::show_info(&ui.window, "Toolchain", &text);
}

/// Shows the editor context menu at the last right-click position, like the
/// Dev-C++ right-click menu.
fn show_context_menu(ui: &Rc<Ui>, x: f64, y: f64) {
    let menu = gio::Menu::new();
    menu.append(Some("Compile"), Some("win.build"));
    menu.append(Some("Compile and Run"), Some("win.run"));
    menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    menu.append(Some("Toggle Breakpoint"), Some("win.toggle-breakpoint"));
    menu.append_item(&gio::MenuItem::new(None, Some("separator")));
    menu.append(Some("Complete Code"), Some("win.complete"));

    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_parent(&ui.window);
    popover.set_has_arrow(false);
    // GTK requires unparenting before the widget is dropped.
    popover.connect_closed(|popover| popover.unparent());
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.max(0.0) as i32,
        y.max(0.0) as i32,
        1,
        1,
    )));
    popover.popup();
}
