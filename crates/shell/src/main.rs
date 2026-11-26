//! megi-rune-shell — an Arc-style Rune UI over the Megi web engine.
//!
//! Run with a display:   cargo run -p shell
//! Headless snapshot:    cargo run -p shell -- --snap shell.png
//! Dump element tree:    cargo run -p shell -- --tree

#![forbid(unsafe_code)]

mod bridge;
mod state;
mod view;

use rune::AppSpec;
use state::State;

fn hide_sidebar(s: &mut State) {
    s.sidebar_open = false;
}

fn main() {
    rune::run(AppSpec::<State> {
        scenes: &[("no-sidebar", hide_sidebar)],
        ..AppSpec::new(
            "Megi — Rune Shell",
            (1280.0, 800.0),
            State::default,
            view::view,
        )
    });
}
