//! GTK4 front-end of the devcpp IDE.
//!
//! Layout mirrors Dev-C++: a menu bar, a tool bar, a notebook of source
//! editors in the centre, and a bottom notebook with the Compiler Output,
//! Debug and Class Browser panels. A side panel shows the project tree.
//!
//! GTK 4.10 deprecated `TreeView`/`ListStore` in favour of `ColumnView`, but
//! the Dev-C++ project tree and class browser are real trees, and `TreeView`
//! is the only widget that models them. The deprecation is therefore
//! intentional and silenced crate-wide.

#![allow(deprecated)]

mod app;
mod editor;
mod panels;

use anyhow::Result;

fn main() -> Result<()> {
    app::run()
}
