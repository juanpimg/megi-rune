//! Aggressive stress tests: try to break the shell the way a user would —
//! hammering controls, toggling the sidebar mid-interaction, resizing to
//! extreme sizes (maximise/minimise), and navigating repeatedly.
//!
//! These use Rune's headless `Runtime` with a real `State` (and therefore a
//! real embedded Megi `WebView`), so a panic or a broken layout surfaces here
//! instead of in front of the user.

use rune::{Env, Key, KeyEvent, Mods, Runtime};

use crate::state::State;
use crate::view::view;

fn rt(size: (f32, f32)) -> Runtime<State> {
    let mut runtime = Runtime::new(State::default(), view, size, Env::new(true, 1.0));
    // `rune::run` wires AppSpec::poll automatically; a bare headless Runtime
    // does not, so register the same callback to mirror the real app exactly.
    runtime.poll = Some(crate::state::poll);
    runtime.on_key = Some(crate::state::on_key);
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
    rt.state.tab_mut().url_edit.replace_text("not a url !!!");
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

#[test]
fn creating_and_switching_tabs_never_panics() {
    let mut rt = rt((1280.0, 800.0));
    // Hammer the new-tab button past the cap; extra clicks must be no-ops.
    for _ in 0..40 {
        if let Some((x, y)) = rt.find("tab-new") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(1.0 / 60.0);
    }
    assert!(
        rt.state.tabs.len() <= crate::state::MAX_TABS,
        "tab count is bounded"
    );
    assert!(
        rt.state.active_tab < rt.state.tabs.len(),
        "active index stays valid"
    );
    // Switch through every tab by clicking its strip element.
    let ids: Vec<u64> = rt.state.tabs.iter().map(|t| t.id).collect();
    for id in ids {
        let key = format!("tab-{id}");
        if let Some((x, y)) = rt.find(&key) {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(1.0 / 60.0);
        assert!(rt.find("page").is_some());
    }
}

#[test]
fn closing_tabs_keeps_at_least_one_and_valid_index() {
    let mut rt = rt((1280.0, 800.0));
    for _ in 0..5 {
        if let Some((x, y)) = rt.find("tab-new") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(1.0 / 60.0);
    }
    let count = rt.state.tabs.len();
    // Click every close button repeatedly, including the last remaining tab.
    for _ in 0..count + 5 {
        let id = rt.state.tabs.first().map(|t| t.id);
        if let Some(id) = id {
            let key = format!("tab-close-{id}");
            if let Some((x, y)) = rt.find(&key) {
                rt.pointer_down(x, y);
                rt.pointer_up(x, y);
            }
        }
        rt.frame(1.0 / 60.0);
        assert!(!rt.state.tabs.is_empty(), "at least one tab survives");
        assert!(rt.state.active_tab < rt.state.tabs.len(), "index valid");
    }
}

#[test]
fn tab_switch_is_non_blocking_and_preserves_each_tabs_url() {
    use std::time::Instant;
    let mut rt = rt((1280.0, 800.0));
    // Open a second tab, then give each a distinct URL string in state.
    rt.state.new_tab();
    let first_id = rt.state.tabs[0].id;
    let second_id = rt.state.tabs[1].id;
    rt.state.tabs[0].loaded_url = "first://page".to_owned();
    rt.state.tabs[1].loaded_url = "second://page".to_owned();
    let start = Instant::now();
    for _ in 0..50 {
        rt.state.select_tab(first_id);
        rt.state.select_tab(second_id);
        rt.frame(1.0 / 240.0);
    }
    assert!(
        start.elapsed().as_millis() < 500,
        "switching tabs blocked the UI thread"
    );
    assert!(rt.state.tabs.iter().any(|t| t.loaded_url == "first://page"));
    assert!(
        rt.state
            .tabs
            .iter()
            .any(|t| t.loaded_url == "second://page")
    );
}

#[test]
fn action_buttons_show_and_expire_toasts() {
    let mut rt = rt((1280.0, 800.0));
    // Every formerly-dead action must now produce visible feedback.
    for key in ["settings", "share", "menu"] {
        if let Some((x, y)) = rt.find(key) {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        rt.frame(1.0 / 60.0);
        assert!(rt.state.toast.is_some(), "{key} produced a toast");
        assert!(rt.find("toast").is_some(), "{key} toast is in the tree");
    }
    // Advance the frame clock past the toast deadline; poll must clear it.
    for _ in 0..400 {
        rt.frame(1.0 / 60.0);
    }
    assert!(rt.state.toast.is_none(), "toast expires on the frame clock");
}

#[test]
fn unicode_titles_never_panic_the_view() {
    // A tab title with multi-byte characters used to be sliced at byte 19,
    // which can land inside a UTF-8 sequence and panic. Build such a title and
    // force a layout through the real view.
    let mut rt = rt((1280.0, 800.0));
    rt.state.tab_mut().title = "Ñandú — historia de España y más texto aquí".to_owned();
    rt.frame(1.0 / 60.0);
    assert!(
        rt.find("page").is_some(),
        "view survives a Unicode tab title"
    );
}

fn chord(c: char) -> KeyEvent {
    KeyEvent {
        key: Key::Command(c),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    }
}

#[test]
fn keyboard_shortcuts_control_tabs_navigation_and_focus() {
    let mut rt = rt((1280.0, 800.0));
    let initial = rt.state.tabs.len();

    // Rune queues keys and dispatches them during frame(), so pump a frame
    // after each chord before asserting.
    let press = |rt: &mut Runtime<State>, ev: KeyEvent| {
        rt.key(ev);
        rt.frame(1.0 / 60.0);
    };

    // Cmd/Ctrl+T opens a tab; Cmd/Ctrl+W closes it again.
    press(&mut rt, chord('t'));
    assert_eq!(rt.state.tabs.len(), initial + 1, "ctrl+t opens a tab");
    press(&mut rt, chord('w'));
    assert_eq!(rt.state.tabs.len(), initial, "ctrl+w closes a tab");

    // Closing never drops the last tab, even if ctrl+w is spammed.
    for _ in 0..8 {
        press(&mut rt, chord('w'));
    }
    assert_eq!(rt.state.tabs.len(), 1, "ctrl+w keeps at least one tab");

    // Cmd/Ctrl+L focuses the address bar without panicking.
    press(&mut rt, chord('l'));

    // Alt+Left / Alt+Right and F5 / Ctrl+R trigger navigation without blocking.
    for ev in [
        KeyEvent {
            key: Key::Left,
            mods: Mods {
                alt: true,
                ..Mods::default()
            },
        },
        KeyEvent {
            key: Key::Right,
            mods: Mods {
                alt: true,
                ..Mods::default()
            },
        },
        KeyEvent {
            key: Key::Function(5),
            mods: Mods::default(),
        },
        chord('r'),
    ] {
        let start = std::time::Instant::now();
        press(&mut rt, ev);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(200),
            "shortcut navigation must stay non-blocking"
        );
    }
    assert!(
        rt.find("page").is_some(),
        "view survives keyboard shortcuts"
    );
}

#[test]
fn tab_titles_are_distinct_per_site() {
    let mut rt = rt((1280.0, 800.0));
    // Two tabs navigated to different sites must show different titles.
    rt.state.new_tab();
    rt.state.navigate("https://github.com/juanpimg/Megi");
    assert_eq!(rt.state.tab().title, "GitHub");
    rt.state.select_tab(1);
    rt.state.new_tab();
    rt.state.navigate("https://www.youtube.com/watch?v=x");
    assert_eq!(rt.state.tab().title, "YouTube");
    assert_ne!(rt.state.tabs[1].title, rt.state.tabs[2].title);
    // An unknown host falls back to a readable hostname, never empty.
    rt.state.navigate("https://example.org/a/b");
    assert_eq!(rt.state.tab().title, "example.org");
}

#[test]
fn address_bar_follows_navigation_and_redirects() {
    // Clicking a bookmark changes the loaded URL; the visible address bar must
    // follow it, otherwise the user sees a stale address. A blocking navigation
    // (with a redirect) must also update what is shown.
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate("https://github.com");
    assert_eq!(
        rt.state.tab().url_edit.text,
        "https://github.com",
        "address bar reflects an async navigation immediately"
    );
    assert!(rt.state.tab().loaded_url.contains("github.com"));

    // The start page is shown as a friendly URL, never as a base64 blob.
    rt.state.navigate("");
    assert_eq!(rt.state.tab().url_edit.text, "https://es.wikipedia.org");

    // A blocking navigation to a data URL with a redirect keeps the field sane.
    rt.state
        .navigate_blocking("data:text/html,<title>x</title><h1>h</h1>");
    assert!(
        !rt.state.tab().url_edit.text.starts_with("data:"),
        "address bar never shows a raw data URL"
    );
}

#[test]
fn loading_state_sets_and_clears_around_navigation() {
    let mut rt = rt((1280.0, 800.0));
    // A data: URL renders fast on the worker; poll until the flag clears.
    rt.state.navigate("data:text/html,<h1>loading test</h1>");
    assert!(
        rt.state.tab().loading,
        "loading is set synchronously on request"
    );
    let mut settled = false;
    for _ in 0..600 {
        rt.frame(1.0 / 60.0);
        if !rt.state.tab().loading {
            settled = true;
            break;
        }
    }
    assert!(settled, "loading clears once the worker result is applied");
    assert!(rt.find("page").is_some());
}

#[test]
fn stale_tab_results_cannot_pollute_a_new_tab() {
    let mut rt = rt((1280.0, 800.0));
    // Queue a navigation, then immediately close that tab and open another.
    rt.state.navigate("https://example.invalid");
    let old_id = rt.state.tab().id;
    rt.state.new_tab();
    let new_id = rt.state.tab().id;
    rt.state.close_tab(old_id);
    for _ in 0..10 {
        rt.frame(1.0 / 60.0);
    }
    // The surviving tab must not show the closed tab's error/url.
    assert_eq!(rt.state.tab().id, new_id);
    assert!(rt.state.tab().nav_error.is_none());
}
