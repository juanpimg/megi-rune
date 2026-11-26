//! The Arc-style shell view: toolbar, left sidebar rail + panels, and the
//! centre pane that paints Megi's rendered page through `rune::Pixels`.

use rune::tok::{self, motion, type_};
use rune::{Color, Cx, El, Key, Shape, col, el, oklch, oklcha, row, text, text_input};

use crate::state::State;

// Palette authored in OKLCH to match the dark, rounded reference chrome.
fn chrome() -> Color {
    oklch(0.24, 0.006, 265.0)
}
fn chrome_dark() -> Color {
    oklch(0.20, 0.006, 265.0)
}
fn panel() -> Color {
    oklch(0.22, 0.006, 265.0)
}
fn hover() -> Color {
    oklcha(1.0, 0.0, 0.0, 0.07)
}
fn muted() -> Color {
    oklch(0.68, 0.008, 265.0)
}
fn accent() -> Color {
    oklch(0.62, 0.16, 275.0)
}

/// First Unicode scalar of a label, or an empty string. Slicing `&s[..1]` would
/// panic on multi-byte first characters (e.g. "Ñandú"), so iterate by chars.
fn first_char(s: &str) -> String {
    s.chars().next().map(String::from).unwrap_or_default()
}

/// Truncates to at most `max` characters, appending an ellipsis when cut.
/// Char-aware so a Unicode title like "Historia de España" cannot be split
/// inside a multi-byte sequence.
fn truncate_chars(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// A small vector icon drawn from an SVG path inside a `size×size` element.
fn icon(path: &'static str, size: f32, color: Color, stroke: bool) -> El {
    let shape = if stroke {
        Shape::new()
            .stroke_svg(path, 2.0, rune::LineCap::Round, color)
            .view_box(24.0, 24.0)
    } else {
        Shape::new().fill_svg(path, color).view_box(24.0, 24.0)
    };
    el().size(size, size).shape(shape)
}

fn sidebar_toggle() -> El {
    icon("M4 4 H20 V20 H4 Z M9 4 V20", 17.0, muted(), true)
}
fn back_icon() -> El {
    icon("M15 5 L8 12 L15 19", 18.0, muted(), true)
}
fn forward_icon() -> El {
    icon("M9 5 L16 12 L9 19", 18.0, muted(), true)
}
fn reload_icon() -> El {
    icon("M20 12 A8 8 0 1 1 17 6 M20 4 V10 H14", 17.0, muted(), true)
}
fn lock_icon() -> El {
    icon(
        "M7 11 V8 A5 5 0 0 1 17 8 V11 M5 11 H19 V20 H5 Z",
        13.0,
        muted(),
        true,
    )
}
fn plus_icon() -> El {
    icon("M12 5 V19 M5 12 H19", 18.0, muted(), true)
}
fn share_icon() -> El {
    icon(
        "M12 16 V4 M8 8 L12 4 L16 8 M5 14 V20 H19 V14",
        17.0,
        muted(),
        true,
    )
}
fn settings_icon() -> El {
    icon(
        "M12 8 A4 4 0 1 0 12 16 A4 4 0 1 0 12 8 M12 2 V5 M12 19 V22 M2 12 H5 M19 12 H22 M5 5 L7 7 M17 17 L19 19 M19 5 L17 7 M7 17 L5 19",
        17.0,
        muted(),
        true,
    )
}
fn menu_icon() -> El {
    icon("M4 7 H20 M4 12 H20 M4 17 H20", 18.0, muted(), true)
}
fn chevron_down() -> El {
    icon("M6 9 L12 15 L18 9", 14.0, muted(), true)
}
fn grid_icon() -> El {
    icon(
        "M4 4 H11 V11 H4 Z M13 4 H20 V11 H13 Z M4 13 H11 V20 H4 Z M13 13 H20 V20 H13 Z",
        17.0,
        muted(),
        true,
    )
}
fn folder_icon() -> El {
    icon("M3 6 H10 L12 9 H21 V19 H3 Z", 17.0, muted(), true)
}
fn gear_icon() -> El {
    icon(
        "M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9 M12 2 V5 M12 19 V22 M2 12 H5 M19 12 H22 M5 5 L7 7 M17 17 L19 19 M19 5 L17 7 M7 17 L5 19",
        17.0,
        muted(),
        true,
    )
}
fn book_icon() -> El {
    icon("M4 5 H20 V19 H4 Z M12 5 V19", 17.0, muted(), true)
}
fn help_icon() -> El {
    icon(
        "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3 M9 9 A3 3 0 0 1 15 9 C15 12 12 12 12 15 M12 18 H12.01",
        17.0,
        muted(),
        true,
    )
}

/// macOS-style traffic lights.
fn traffic_lights() -> El {
    row()
        .gap(8.0)
        .items_center()
        .child(
            el().size(12.0, 12.0)
                .radius(6.0)
                .fill(oklch(0.72, 0.19, 25.0)),
        )
        .child(
            el().size(12.0, 12.0)
                .radius(6.0)
                .fill(oklch(0.78, 0.17, 85.0)),
        )
        .child(
            el().size(12.0, 12.0)
                .radius(6.0)
                .fill(oklch(0.66, 0.17, 150.0)),
        )
}

/// A toolbar button with hover feedback. Handlers receive `Tx` so actions can
/// timestamp toasts with Rune's frame clock.
fn tool_button(key: &'static str, glyph: El, action: fn(&mut State, f64)) -> El {
    el().key(key)
        .role("button")
        .size(30.0, 30.0)
        .radius(8.0)
        .center()
        .fill(Color::TRANSPARENT)
        .hover_fill(hover())
        .motion(motion::SNAPPY)
        .on_press_tx(move |s: &mut State, tx: &mut rune::Tx| action(s, tx.clock()))
        .child(glyph)
}

fn action_toggle_sidebar(s: &mut State, _now: f64) {
    s.sidebar_open = !s.sidebar_open;
}
fn action_back(s: &mut State, _now: f64) {
    s.back();
}
fn action_forward(s: &mut State, _now: f64) {
    s.forward();
}
fn action_reload(s: &mut State, _now: f64) {
    s.reload();
}
fn action_new_tab(s: &mut State, _now: f64) {
    s.new_tab();
}
fn action_share(s: &mut State, now: f64) {
    s.show_toast("Compartir: enlace copiado", now, 2.5);
}
fn action_settings(s: &mut State, now: f64) {
    s.show_toast("Ajustes: próximamente", now, 2.5);
}
fn action_menu(s: &mut State, now: f64) {
    s.show_toast("Menú: próximamente", now, 2.5);
}
fn action_folders(s: &mut State, now: f64) {
    s.show_toast("Carpetas: próximamente", now, 2.5);
}
fn action_books(s: &mut State, now: f64) {
    s.show_toast("Lecturas: próximamente", now, 2.5);
}
fn action_help(s: &mut State, now: f64) {
    s.show_toast("Ayuda: Megi + Rune", now, 2.5);
}

/// Central capsule address bar.
fn address_bar(s: &State) -> El {
    row()
        .key("address-bar")
        .h(32.0)
        .grow(1.0)
        .max_w(560.0)
        .radius(10.0)
        .fill(chrome_dark())
        .border(1.0, oklcha(1.0, 0.0, 0.0, 0.06))
        .items_center()
        .px(12.0)
        .gap(8.0)
        .child(lock_icon())
        .child(
            text_input(&s.tab().url_edit, type_::BODY)
                .key("url-input")
                .grow(1.0)
                .color(muted())
                .on_input_key(|st: &mut State, ev: &rune::KeyEvent, _tx| {
                    if ev.key == Key::Enter {
                        st.commit_url_bar();
                        return true;
                    }
                    false
                }),
        )
}

fn toolbar(s: &State) -> El {
    let bar = row()
        .key("toolbar")
        .h(52.0)
        .w_full()
        .fill(chrome())
        .items_center()
        .px(16.0)
        .gap(14.0)
        .child(traffic_lights())
        .child(tool_button(
            "toggle-sidebar",
            sidebar_toggle(),
            action_toggle_sidebar,
        ))
        .child(tool_button("back", back_icon(), action_back))
        .child(tool_button("forward", forward_icon(), action_forward))
        .child(tool_button("reload", reload_icon(), action_reload))
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .justify_center()
                .child(address_bar(s)),
        );

    // On narrow windows the four trailing actions do not fit. The icon rail
    // already has settings/share/new-tab, so drop these from the toolbar to
    // keep the address bar usable.
    if s.viewport_w >= SIDEBAR_PANEL_MIN_W {
        bar.child(tool_button("settings", settings_icon(), action_settings))
            .child(tool_button("share", share_icon(), action_share))
            .child(tool_button("new-tab", plus_icon(), action_new_tab))
            .child(tool_button("menu", menu_icon(), action_menu))
    } else {
        bar
    }
}

/// A row of browser tabs between the toolbar and the content. Each tab shows a
/// short title, is clickable to switch instantly, and has a small close button.
fn tab_strip(s: &State) -> El {
    let mut strip = row()
        .key("tab-strip")
        .w_full()
        .h(38.0)
        .fill(chrome_dark())
        .items_center()
        .px(12.0)
        .gap(6.0);

    for (i, tab) in s.tabs.iter().enumerate() {
        let active = i == s.active_tab;
        let id = tab.id;
        let title = truncate_chars(&tab.title, 20);
        // A single keyed element per tab: pressing it selects the tab (instant
        // cached-pixel swap). The close "×" is a separate element inside with
        // its own handler and key so tests and users can target either.
        strip = strip.child(
            el().key(format!("tab-{id}"))
                .role("button")
                .h(28.0)
                .min_w(110.0)
                .max_w(180.0)
                .radius(8.0)
                .center()
                .fill(if active { panel() } else { Color::TRANSPARENT })
                .hover_fill(hover())
                .motion(motion::SNAPPY)
                .on_press(move |st: &mut State| st.select_tab(id))
                .child(
                    row()
                        .items_center()
                        .gap(8.0)
                        .px(10.0)
                        .child(
                            text(title)
                                .font(type_::LABEL)
                                .color(if active {
                                    tok::theme(true).text
                                } else {
                                    muted()
                                })
                                .truncate(),
                        )
                        .child(
                            el().key(format!("tab-close-{id}"))
                                .role("button")
                                .size(16.0, 16.0)
                                .radius(8.0)
                                .center()
                                .hover_fill(hover())
                                .on_press(move |st: &mut State| st.close_tab(id))
                                .child(text("×").font(type_::LABEL).color(muted())),
                        ),
                ),
        );
    }

    // "+" button to add a tab.
    strip = strip.child(tool_button("tab-new", plus_icon(), action_new_tab));
    strip
}

/// The narrow vertical icon rail on the far left.
fn rail() -> El {
    col()
        .key("rail")
        .w(52.0)
        .h_full()
        .fill(chrome_dark())
        .items_center()
        .py(14.0)
        .gap(12.0)
        // top group
        .child(tool_button(
            "rail-spaces",
            grid_icon(),
            action_toggle_sidebar,
        ))
        .child(tool_button("rail-folders", folder_icon(), action_folders))
        .child(tool_button("rail-share", share_icon(), action_share))
        .child(tool_button("rail-new", plus_icon(), action_new_tab))
        .child(el().grow(1.0))
        // bottom group
        .child(tool_button("rail-settings", gear_icon(), action_settings))
        .child(tool_button("rail-books", book_icon(), action_books))
        .child(tool_button("rail-help", help_icon(), action_help))
        .child(
            el().key("avatar")
                .size(30.0, 30.0)
                .radius(15.0)
                .center()
                .fill(accent())
                .child(text("J").font(type_::LABEL).color(Color::WHITE)),
        )
}

fn section_header(label: &'static str, key: &'static str) -> El {
    row()
        .key(key)
        .w_full()
        .items_center()
        .justify_between()
        .pt(6.0)
        .pb(8.0)
        .child(text(label).font(type_::SECTION).color(muted()))
        .child(chevron_down())
}

/// A circular space/favourite tile.
fn round_tile(
    glyph: impl Into<String>,
    tint: u32,
    label: &'static str,
    on_press: impl Fn(&mut State) + 'static,
) -> El {
    let c = Color {
        r: ((tint >> 16) & 0xff) as f32 / 255.0,
        g: ((tint >> 8) & 0xff) as f32 / 255.0,
        b: (tint & 0xff) as f32 / 255.0,
        a: 1.0,
    };
    el().role("button")
        .label(label)
        .size(40.0, 40.0)
        .radius(20.0)
        .center()
        .fill(c)
        .motion(motion::SNAPPY)
        .on_press(on_press)
        .child(text(glyph).font(type_::TITLE).color(Color::WHITE))
}

fn tile_row(prefix: &'static str, tiles: Vec<El>) -> El {
    row().key(prefix).w_full().gap(10.0).children(tiles)
}

/// Space tiles built from state, with the active space ringed in accent.
/// Both states carry an `outline` of identical width so switching spaces never
/// changes the tile's layout box (a border on only the active tile made the
/// whole row jump 4 px on every click).
fn space_tiles(s: &State) -> Vec<El> {
    s.spaces
        .iter()
        .enumerate()
        .map(|(i, sp)| {
            let active = i == s.active_space;
            let idx = i;
            round_tile(
                first_char(sp.label),
                sp.tint,
                sp.label,
                move |st: &mut State| {
                    st.active_space = idx;
                },
            )
            .outline(2.0, 1.0, if active { accent() } else { Color::TRANSPARENT })
        })
        .collect()
}

/// Favourite tiles built from state, split into two rows of four.
fn favourite_tiles(s: &State) -> (Vec<El>, Vec<El>) {
    let make = |f: &crate::state::Favourite| {
        let url = f.url;
        round_tile(f.glyph, f.tint, f.label, move |st: &mut State| {
            st.navigate(url)
        })
    };
    let first = s.favourites.iter().take(4).map(make).collect();
    let second = s.favourites.iter().skip(4).map(make).collect();
    (first, second)
}

/// A bookmark list item with a small favicon square.
fn bookmark(label: &'static str, tint: u32, url: &'static str, key: &'static str) -> El {
    let c = Color {
        r: ((tint >> 16) & 0xff) as f32 / 255.0,
        g: ((tint >> 8) & 0xff) as f32 / 255.0,
        b: (tint & 0xff) as f32 / 255.0,
        a: 1.0,
    };
    el().key(key)
        .role("button")
        .w_full()
        .h(34.0)
        .radius(8.0)
        .items_center()
        .px(8.0)
        .gap(10.0)
        .fill(Color::TRANSPARENT)
        .hover_fill(hover())
        .on_press(move |st: &mut State| st.navigate(url))
        .child(
            el().size(18.0, 18.0).radius(4.0).center().fill(c).child(
                text(first_char(label))
                    .font(type_::LABEL)
                    .color(Color::WHITE),
            ),
        )
        .child(text(label).font(type_::BODY).color(tok::theme(true).text))
}

/// Wide sidebar panel with spaces, favourites and bookmarks.
fn sidebar_panel(s: &State) -> El {
    col()
        .key("sidebar-panel")
        .w(232.0)
        .h_full()
        .fill(panel())
        .px(16.0)
        .py(16.0)
        .gap(2.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec::default())
        // Spaces
        .child(section_header("Espacios", "spaces-header"))
        .child(tile_row("spaces-row", space_tiles(s)))
        .child(el().h(14.0))
        .child(el().w_full().h(1.0).fill(oklcha(1.0, 0.0, 0.0, 0.08)))
        // Favourites
        .child(section_header("Favoritos", "favs-header"))
        .child(tile_row("favs-row-1", favourite_tiles(s).0))
        .child(el().h(10.0))
        .child(tile_row("favs-row-2", favourite_tiles(s).1))
        .child(el().h(14.0))
        .child(el().w_full().h(1.0).fill(oklcha(1.0, 0.0, 0.0, 0.08)))
        .child(el().h(8.0))
        // Bookmarks
        .child(bookmark(
            "Historia de España",
            0xc0392b,
            "https://es.wikipedia.org",
            "bm-historia",
        ))
        .child(bookmark(
            "Andalucía - Wikipedia",
            0x2b2b2b,
            "https://es.wikipedia.org/wiki/Andalucía",
            "bm-andalucia",
        ))
        .child(bookmark(
            "Diseño de Interfaces",
            0x6741d9,
            "https://es.wikipedia.org/wiki/Diseño_de_interfaces",
            "bm-diseno",
        ))
        .child(bookmark(
            "Desarrollo de Software",
            0x1a1a1a,
            "https://es.wikipedia.org/wiki/Desarrollo_de_software",
            "bm-dev",
        ))
}

/// The centre pane: the Megi-rendered page painted as a Rune image, on a white
/// card with rounded corners, plus a thin status line for errors.
fn content_pane(s: &State) -> El {
    let page = el()
        .key("page")
        .w_full()
        .grow(1.0)
        .radius(8.0)
        // `Fit` preserves the engine frame's aspect ratio, so resizing the
        // window letterboxes instead of stretching the article sideways.
        .pixels_fit(&s.tab().page, rune::SurfaceFit::Fit)
        // Pointing-hand cursor while hovering a rendered link.
        .cursor(if s.hover_link {
            rune::Cursor::Pointer
        } else {
            rune::Cursor::Default
        })
        .min_h(0.0)
        // Record where the page is laid out so a click can be mapped back into
        // CSS pixels (the Fit letterbox is computed in `State::link_at_local`).
        .on_layout(|st: &mut State, r: [f32; 4]| st.page_rect = r)
        // Track hover over links so the pointer becomes a hand, like a browser.
        .on_pointer_move_local(|st: &mut State, p: rune::PointerEvent, _tx| {
            st.hover_link = st.link_at_local(p.x, p.y).is_some();
        })
        .on_hover_change(|st: &mut State, inside: bool, _tx| {
            if !inside {
                st.hover_link = false;
            }
        })
        // Clicking a rendered link navigates, just like a real browser. The
        // hit-test is pure state work — no engine render on the UI thread.
        // Right-click opens our in-app context menu anchored at the pointer.
        .on_secondary_press_local(|st: &mut State, p: rune::PointerEvent, _tx| {
            if let Some(url) = st.link_at_local(p.x, p.y) {
                st.open_context_menu(&url, p.window_x, p.window_y);
            } else {
                st.close_context_menu();
            }
        })
        // Primary click navigates in place; Cmd/Ctrl+click and middle-click
        // open the link in a new tab.
        .on_any_press(
            |st: &mut State, btn, p: rune::PointerEvent, tx: &mut rune::Tx| {
                if btn == rune::PointerButton::Secondary {
                    return; // handled by `on_secondary_press`
                }
                let new_tab = btn == rune::PointerButton::Middle
                    || (btn == rune::PointerButton::Primary && tx.mods().primary());
                if let Some(url) = st.link_at_local(p.x, p.y) {
                    st.close_context_menu();
                    if new_tab {
                        st.new_tab_with_url(&url);
                    } else {
                        st.navigate(&url);
                    }
                }
            },
        );

    let mut pane = col()
        .key("content")
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .fill(oklch(0.30, 0.005, 265.0))
        .padding(10.0)
        .gap(8.0)
        .child(page);

    if let Some(err) = &s.tab().nav_error {
        pane = pane.child(
            text(format!("No se pudo cargar: {}", err))
                .font(type_::LABEL)
                .color(oklch(0.75, 0.15, 30.0)),
        );
    }
    if let Some((message, _)) = &s.toast {
        pane = pane.child(
            el().key("toast")
                .role("status")
                .radius(8.0)
                .px(12.0)
                .py(8.0)
                .fill(accent())
                .child(text(message).font(type_::LABEL).color(Color::WHITE)),
        );
    }
    pane
}

/// Below this width the wide sidebar panel gives way to the icon rail only,
/// so the page keeps a usable share of a narrow window.
const SIDEBAR_PANEL_MIN_W: f32 = 520.0;

/// Root view.
/// The in-app link context menu, positioned at the right-click anchor in
/// window points. Out of flow (`absolute`) so it overlays the page without
/// displacing any chrome. Only built while a menu is open.
fn context_menu(s: &State) -> El {
    let Some((_, x, y)) = &s.context_menu else {
        return el().key("context-menu-none");
    };
    let menu_item = |label: &'static str, key: &'static str, action: fn(&mut State, f64)| {
        el().key(key)
            .role("menuitem")
            .w(190.0)
            .px(12.0)
            .py(9.0)
            .radius(6.0)
            .hover_fill(hover())
            .motion(motion::SNAPPY)
            .on_press_tx(move |st: &mut State, tx: &mut rune::Tx| action(st, tx.clock()))
            .child(text(label).font(type_::LABEL).color(Color::WHITE))
    };
    col()
        .key("context-menu")
        .role("menu")
        .absolute(*x, *y)
        .w(200.0)
        .padding(5.0)
        .radius(10.0)
        .fill(panel())
        .shadow(8.0, 24.0, oklcha(0.0, 0.0, 0.0, 0.45))
        // Clicking the menu itself must not fall through to the page handler.
        .on_any_press(|_: &mut State, _, _, _| {})
        .child(menu_item(
            "Abrir en nueva pestaña",
            "ctx-new-tab",
            action_ctx_new_tab,
        ))
        .child(menu_item("Copiar dirección", "ctx-copy", action_ctx_copy))
}

fn action_ctx_new_tab(s: &mut State, _now: f64) {
    s.context_open_in_new_tab();
}
fn action_ctx_copy(s: &mut State, now: f64) {
    s.context_copy_url(now);
}

pub fn view(s: &State, _cx: &mut Cx<State>) -> El {
    // An explicit sidebar toggle always wins; otherwise auto-collapse the wide
    // panel on narrow windows while keeping the icon rail for navigation.
    let panel_visible = s.sidebar_open && s.viewport_w >= SIDEBAR_PANEL_MIN_W;
    let sidebar = if s.sidebar_open {
        if panel_visible {
            row().key("sidebar").child(rail()).child(sidebar_panel(s))
        } else {
            // Narrow window: keep the rail (still lets the user toggle), drop
            // the wide panel so it cannot eat the whole viewport.
            row().key("sidebar").child(rail())
        }
    } else {
        el().key("sidebar-hidden")
    };

    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(chrome_dark())
        // Report the real window size back into state for responsive decisions.
        .on_layout(|st: &mut State, r| {
            st.viewport_w = r[2];
            st.viewport_h = r[3];
        })
        .child(toolbar(s))
        // A 2px accent line under the tabs shows a load in progress. It only
        // exists while loading, so it costs nothing in the steady state.
        .child(if s.tab().loading {
            el().key("loading-bar").h(2.0).w_full().fill(accent())
        } else {
            el().key("loading-bar-idle").h(2.0).w_full()
        })
        .child(tab_strip(s))
        .child(
            row()
                .key("body")
                .grow(1.0)
                .min_h(0.0)
                .child(sidebar)
                .child(content_pane(s)),
        )
        .child(context_menu(s))
}
