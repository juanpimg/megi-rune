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
fn link_regions_are_extracted_and_resolved() {
    // A rendered page with a real absolute link must yield a clickable region
    // in CSS pixels, and `link_at_local` must resolve a click on it.
    let mut st = crate::state::State::default();
    st.navigate_blocking(
        "data:text/html,<a href='https://example.org/dest' style='font-size:40px'>ENLACE</a>",
    );
    assert!(
        !st.tab().links.is_empty(),
        "a rendered <a> yields link regions"
    );
    let link = st.tab().links[0].clone();
    assert_eq!(link.href, "https://example.org/dest");

    // Simulate a click exactly at the link's centre with a page element that
    // fills the frame at 1:1 (page_rect == frame size, placed at origin).
    st.page_rect = [0.0, 0.0, st.tab().page_w as f32, st.tab().page_h as f32];
    let cx = link.x + link.w * 0.5;
    let cy = link.y + link.h * 0.5;
    assert_eq!(
        st.link_at_local(cx, cy).as_deref(),
        Some("https://example.org/dest"),
        "clicking the link centre resolves its absolute href"
    );
    assert!(
        st.link_at_local(-100.0, -100.0).is_none(),
        "clicks outside the links hit nothing"
    );
}

#[test]
fn hit_testing_respects_fit_letterbox() {
    // When the page element is wider than the frame's aspect ratio, Fit adds
    // horizontal letterbox bars. A click must be shifted by that offset before
    // hitting the link, otherwise the mapping is wrong.
    let mut st = crate::state::State::default();
    st.navigate_blocking(
        "data:text/html,<a href='https://example.org/x' style='font-size:40px'>X</a>",
    );
    let link = st.tab().links[0].clone();
    let (sw, sh) = (st.tab().page_w as f32, st.tab().page_h as f32);
    // Element is twice as wide as the frame → scale = min(2, 1) = 1,
    // drawn w = sw, so the frame is centred with (nw - sw)/2 on the left.
    let nw = sw * 2.0;
    let nh = sh;
    st.page_rect = [0.0, 0.0, nw, nh];
    let local_x = (nw - sw) * 0.5 + link.x + link.w * 0.5;
    let local_y = link.y + link.h * 0.5;
    assert_eq!(
        st.link_at_local(local_x, local_y).as_deref(),
        Some("https://example.org/x"),
        "letterbox offset is accounted for"
    );
}

#[test]
fn relative_links_resolve_against_base() {
    let base = "https://es.wikipedia.org/wiki/Historia_de_España";
    assert_eq!(
        crate::state::resolve_href(base, "/wiki/Andalucía"),
        "https://es.wikipedia.org/wiki/Andalucía"
    );
    assert_eq!(
        crate::state::resolve_href(base, "Fenicia"),
        "https://es.wikipedia.org/wiki/Fenicia"
    );
    assert_eq!(
        crate::state::resolve_href(base, "//otro.example.com/x"),
        "https://otro.example.com/x"
    );
    assert_eq!(
        crate::state::resolve_href(base, "https://absolute.test/z"),
        "https://absolute.test/z"
    );
}

#[test]
fn clicking_page_link_navigates_in_real_view() {
    // End-to-end: render a page with a link through the real view, click it at
    // the correct window position, and check the tab navigates to the href.
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate_blocking(
        "data:text/html,<a href='https://example.org/go' style='font-size:48px'>GO</a>",
    );
    rt.frame(1.0 / 60.0); // let on_layout report the page rect
    let link = rt.state.tab().links[0].clone();
    let [px, py, pw, ph] = rt.state.page_rect;
    assert!(pw > 0.0 && ph > 0.0, "page element has a laid-out rect");
    let (sw, sh) = (rt.state.tab().page_w as f32, rt.state.tab().page_h as f32);
    let scale = (pw / sw).min(ph / sh);
    let origin_x = px + (pw - sw * scale) * 0.5;
    let origin_y = py + (ph - sh * scale) * 0.5;
    let click_x = origin_x + (link.x + link.w * 0.5) * scale;
    let click_y = origin_y + (link.y + link.h * 0.5) * scale;
    rt.pointer_down(click_x, click_y);
    rt.pointer_up(click_x, click_y);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state.tab().loaded_url,
        "https://example.org/go",
        "clicking a rendered link navigates the active tab"
    );
}

#[test]
fn right_click_opens_context_menu_with_actions() {
    // Right-clicking a rendered link opens an in-app context menu whose two
    // actions (new tab / copy URL) are observable in state and testable headlessly.
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate_blocking(
        "data:text/html,<a href='https://example.org/ctx' style='font-size:48px'>GO</a>",
    );
    rt.frame(1.0 / 60.0);
    let link = rt.state.tab().links[0].clone();
    let [px, py, pw, ph] = rt.state.page_rect;
    let (sw, sh) = (rt.state.tab().page_w as f32, rt.state.tab().page_h as f32);
    let scale = (pw / sw).min(ph / sh);
    let cx = px + (pw - sw * scale) * 0.5 + (link.x + link.w * 0.5) * scale;
    let cy = py + (ph - sh * scale) * 0.5 + (link.y + link.h * 0.5) * scale;

    rt.secondary_down(cx, cy);
    rt.secondary_up(cx, cy);
    rt.frame(1.0 / 60.0);
    assert!(
        rt.state.context_menu.is_some(),
        "right-click opened the menu"
    );
    assert!(
        rt.find("context-menu").is_some(),
        "menu is in the view tree"
    );
    assert!(rt.find("ctx-new-tab").is_some(), "new-tab item present");
    assert!(rt.find("ctx-copy").is_some(), "copy item present");

    // Escape dismisses an open menu; reopen it for the action checks.
    rt.key(KeyEvent::from(Key::Escape));
    rt.frame(1.0 / 60.0);
    assert!(rt.state.context_menu.is_none(), "Escape dismissed the menu");
    rt.secondary_down(cx, cy);
    rt.secondary_up(cx, cy);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.context_menu.is_some(), "menu reopened");

    // "Copiar dirección" stores the URL and dismisses the menu.
    let (ix, iy) = rt.find("ctx-copy").unwrap();
    rt.pointer_down(ix, iy);
    rt.pointer_up(ix, iy);
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state.last_copied_url.as_deref(),
        Some("https://example.org/ctx")
    );
    assert!(rt.state.context_menu.is_none(), "menu closed after action");
}

#[test]
fn cmd_click_opens_link_in_new_tab() {
    use rune::{Mods, PhysicalInput};
    // Cmd/Ctrl+click on a rendered link opens a new tab instead of navigating
    // the current one, leaving the original tab's URL untouched.
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate_blocking(
        "data:text/html,<a href='https://example.org/new' style='font-size:48px'>GO</a>",
    );
    rt.frame(1.0 / 60.0);
    let original_id = rt.state.tab().id;
    let link = rt.state.tab().links[0].clone();
    let [px, py, pw, ph] = rt.state.page_rect;
    let (sw, sh) = (rt.state.tab().page_w as f32, rt.state.tab().page_h as f32);
    let scale = (pw / sw).min(ph / sh);
    let cx = px + (pw - sw * scale) * 0.5 + (link.x + link.w * 0.5) * scale;
    let cy = py + (ph - sh * scale) * 0.5 + (link.y + link.h * 0.5) * scale;

    rt.physical_input(PhysicalInput::Modifiers(Mods {
        cmd: true,
        ..Mods::default()
    }));
    rt.pointer_down(cx, cy);
    rt.pointer_up(cx, cy);
    rt.frame(1.0 / 60.0);

    assert_eq!(rt.state.tabs.len(), 2, "a new tab was opened");
    // The new tab is active and requested the link URL (async, so loaded_url
    // reflects the request even before the worker's pixels arrive).
    assert_eq!(rt.state.tab().loaded_url, "https://example.org/new");
    // The original tab still sits at its own URL.
    let original = rt.state.tabs.iter().find(|t| t.id == original_id).unwrap();
    assert!(
        original.loaded_url.starts_with("data:"),
        "original tab untouched"
    );
}

#[test]
fn hovering_a_link_shows_pointer_cursor() {
    use rune::Cursor;
    // Hovering a rendered link must flip the page to a pointing-hand cursor,
    // and leaving it must restore the default cursor.
    let mut rt = rt((1280.0, 800.0));
    rt.state.navigate_blocking(
        "data:text/html,<a href='https://example.org/go' style='font-size:48px'>GO</a>",
    );
    rt.frame(1.0 / 60.0);
    let link = rt.state.tab().links[0].clone();
    let [px, py, pw, ph] = rt.state.page_rect;
    let (sw, sh) = (rt.state.tab().page_w as f32, rt.state.tab().page_h as f32);
    let scale = (pw / sw).min(ph / sh);
    let in_x = px + (pw - sw * scale) * 0.5 + (link.x + link.w * 0.5) * scale;
    let in_y = py + (ph - sh * scale) * 0.5 + (link.y + link.h * 0.5) * scale;

    rt.pointer_move(in_x, in_y);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.hover_link, "hover state set over a link");
    assert_eq!(rt.cursor(), Cursor::Pointer, "hand cursor over a link");

    // Move the pointer far outside the page (to the toolbar) and settle.
    rt.pointer_move(2.0, 2.0);
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.hover_link, "hover state cleared off a link");
    assert_eq!(rt.cursor(), Cursor::Default, "default cursor off a link");
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
