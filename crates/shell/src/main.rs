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

/// Deterministic scene: renders a small inline page through the real engine,
/// useful for verifying the bridge without depending on the network.
fn inline_page(s: &mut State) {
    s.navigate_blocking("data:text/html,<h1 style='font:48px sans-serif'>Puente OK</h1>");
}

/// Deterministic scene: opens the link context menu at a fixed spot so the
/// overlay can be snapshotted and reviewed visually.
fn context_menu_scene(s: &mut State) {
    s.open_context_menu(
        "https://es.wikipedia.org/wiki/Historia_de_España",
        320.0,
        260.0,
    );
}

fn main() {
    rune::run(AppSpec::<State> {
        scenes: &[
            ("no-sidebar", hide_sidebar),
            ("inline-page", inline_page),
            ("context-menu", context_menu_scene),
        ],
        poll: Some(state::poll),
        on_key: Some(state::on_key),
        ..AppSpec::new(
            "Megi — Rune Shell",
            (1280.0, 800.0),
            State::default,
            view::view,
        )
    });
}

#[cfg(test)]
mod stress;
