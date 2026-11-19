//! Orthodox Navigator: a two-panel console file manager in the spirit of DOS Navigator.
#![allow(clippy::too_many_arguments)]

mod app;
mod archive;
mod calc;
mod dialogs;
mod diff;
mod editor;
mod favorites;
mod fileops;
mod find;
mod format;
mod game;
mod help;
mod history;
mod git;
mod menu;
mod panel;
mod platform;
mod preview;
mod rename;
mod screen;
mod syntax;
mod sysmon;
mod theme;
mod tools;
mod tree;
mod util;
mod viewer;
mod vscode;

use std::io::IsTerminal;

fn main() {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!("Orthodox Navigator needs an interactive terminal.");
        eprintln!("Run it directly in a terminal window (not through a pipe or another tool).");
        std::process::exit(2);
    }

    // Ctrl+C must not kill us while a command runs in the console. A handler (unlike "ignore")
    // is not inherited by child processes, so the command itself still gets Ctrl+C normally.
    let _ = ctrlc::set_handler(|| {});

    // Always give the terminal back, even on a panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        screen::restore_terminal();
        default_hook(info);
    }));

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut app = app::App::new(&args);
    let result = app.run();
    app.screen.shutdown();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
