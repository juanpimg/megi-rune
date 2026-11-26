//! Aggressive stress tests: try to break the shell the way a user would —
//! hammering controls, toggling the sidebar mid-interaction, resizing to
//! extreme sizes (maximise/minimise), and navigating repeatedly.
//!
//! These use Rune's headless `Runtime` with a real `State` (and therefore a
//! real embedded Megi `WebView`), so a panic or a broken layout surfaces here
//! instead of in front of the user.

use rune::{Env, Key, KeyEvent, Runtime};

use crate::state::State;
use crate::view::view;

fn rt(size: (f32, f32)) -> Runtime<State> {
    let mut runtime = Runtime::new(State::default(), view, size, Env::new(true, 1.0));
    runtime.frame(0.0);
    runtime
}

/// Presses every keyed button present in the tree, one after another, with a
/// frame between each so springs/hover state settle. Missing keys are skipped:
/// the sidebar can be hidden, so its controls legitimately disappear.
fn press_present(rt: &mut Runtime<State>, keys: &[&'static str]) {
    for key in keys {
        if let Some((x, y)) = rt.find(key) {
            rt.pointer_move(x, y);
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
        }
    }
}

#[test]
fn every_toolbar_control_survives_a_hammering() {
    let mut rt = rt((1280.0, 800.0));
    let toolbar_keys = [
        "toggle-sidebar",
        "back",
        "forward",
        "reload",
        "settings",
        "share",
        "new-tab",
        "menu",
    ];
    // Three full passes: back/forward/reload change engine state, and the
    // sidebar toggle changes the tree shape between presses.
    for _ in 0..3 {
        press_present(&mut rt, &toolbar_keys);
    }
    // Rune must still be able to lay out and present the root afterwards.
    assert!(rt.find("root").is_some(), "root survives repeated clicks");
}

#[test]
fn toggling_sidebar_mid_click_never_panics() {
    let mut rt = rt((1280.0, 800.0));
    for _ in 0..40 {
        // A real toggle flips the tree between rail+panel and hidden.
        if let Some((x, y)) = rt.find("toggle-sidebar") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        // Immediately try to click a sidebar control that may have vanished.
        if let Some((x, y)) = rt.find("bm-historia") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("root").is_some());
}

#[test]
fn maximise_and_minimise_extremes_do_not_crash() {
    // A spread from tiny to 4K/ultrawide, including aspect ratios far from the
    // design size. Mirrors window maximise, un-maximise and aggressive resizes.
    let sizes: [(f32, f32); 12] = [
        (1280.0, 800.0),
        (320.0, 200.0),
        (1.0, 1.0),
        (3840.0, 2160.0),
        (2560.0, 1080.0),
        (800.0, 1200.0),
        (1200.0, 400.0),
        (400.0, 1200.0),
        (1024.0, 768.0),
        (1920.0, 1080.0),
        (600.0, 600.0),
        (1280.0, 800.0),
    ];
    let mut rt = rt(sizes[0]);
    for &(w, h) in &sizes {
        rt.set_viewport((w, h), 1.0);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("root").is_some(), "root present at {w}x{h}");
    }
}

#[test]
fn resize_while_sidebar_hidden_and_shown() {
    let mut rt = rt((1280.0, 800.0));
    for i in 0..30 {
        let w = 200.0 + (i as f32 * 137.0) % 3800.0;
        let h = 120.0 + (i as f32 * 91.0) % 2000.0;
        if i % 3 == 0
            && let Some((x, y)) = rt.find("toggle-sidebar")
        {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.set_viewport((w, h), 1.0);
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("root").is_some());
}

#[test]
fn rapid_navigation_via_bookmarks_and_tiles() {
    let mut rt = rt((1280.0, 800.0));
    let nav_keys = [
        "bm-historia",
        "bm-andalucia",
        "bm-diseno",
        "bm-dev",
        "rail-spaces",
        "back",
        "forward",
    ];
    // Bookmarks point at real https URLs; with no network these produce load
    // errors that must be handled gracefully (no panic, page still presented).
    for _ in 0..4 {
        press_present(&mut rt, &nav_keys);
    }
    assert!(
        rt.find("page").is_some(),
        "page element survives navigation errors"
    );
}

#[test]
fn scroll_and_wheel_never_panic() {
    let mut rt = rt((900.0, 600.0));
    for i in 0..20 {
        rt.wheel(100.0, 100.0, 240.0);
        rt.pointer_move(100.0 + i as f32, 100.0);
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("sidebar-panel").is_some());
}

#[test]
fn url_bar_enter_with_garbage_is_survivable() {
    let mut rt = rt((1280.0, 800.0));
    // Rune routes keys to the focused control, so focus the URL input first.
    rt.update(|_, tx| tx.focus("url-input", false));
    rt.frame(0.0);
    // Garbage input: committing it must not panic or freeze. Because navigation
    // is now asynchronous, a load error only appears after poll drains the
    // worker result, so give it a few frames.
    rt.state.url_edit.replace_text("not a url !!!");
    rt.key(KeyEvent::from(Key::Enter));
    for _ in 0..5 {
        rt.frame(1.0 / 60.0);
    }
    // The important invariant: the shell stays alive and the page is shown.
    assert!(
        rt.find("page").is_some(),
        "page element survives invalid URL"
    );
    assert!(rt.find("root").is_some());
}

#[test]
fn space_switching_rapidly_keeps_active_index_valid() {
    let mut rt = rt((1280.0, 800.0));
    // Space tiles have no key; drive the state directly to simulate rapid
    // workspace switching, then force a layout each time.
    for i in 0..50 {
        rt.state.active_space = i % rt.state.spaces.len();
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.find("root").is_some());
}

#[test]
fn webview_and_frame_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<megi_webview::WebView>();
    assert_send::<megi_webview::WebFrame>();
}

#[test]
fn navigation_clicks_are_non_blocking() {
    use std::time::Instant;
    let mut rt = rt((1280.0, 800.0));
    // A click on a bookmark must return immediately (it only queues a request
    // to the worker); the expensive render happens off the UI thread.
    let keys = ["bm-historia", "bm-andalucia", "bm-diseno"];
    let start = Instant::now();
    press_present(&mut rt, &keys);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "clicking 3 bookmarks blocked the UI thread for {:?}",
        elapsed
    );
}

#[test]
fn blocking_navigation_updates_page_for_snapshots() {
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate_blocking("data:text/html,<h1>OK</h1>");
    rt.frame(0.0);
    assert!(rt.find("page").is_some());
}
