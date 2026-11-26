# megi-rune

An alternative, Arc-style UI shell for the [Megi](https://github.com/juanpimg/Megi)
web engine, built with [Rune](https://github.com/anomalyco/rune).

This repository is intentionally separate from Megi: the engine's own
`megi-browser` shell (winit + softbuffer) is left untouched. Instead this shell
depends only on the embeddable `megi-webview` crate and paints its rendered
frames through Rune's GPU renderer.

## How it works

Megi's `WebView` is a platform-agnostic engine: it renders a page into an owned
RGBA8 `Pixmap` and exposes navigation + hit-testing, with no window system of
its own. The bridge is deliberately tiny:

```
WebFrame.pixmap.to_rgba_bytes()  ->  rune::Pixels::set(w, h, rgba)  ->  el().pixels(&px)
```

No FFI, no `unsafe`, no engine patches — just one `Vec<u8>` copy per navigation.
For a 1120×820 viewport that is ~3.7 MB per load.

Megi's `load()` renders **synchronously** and can block on the network, so
calling it from a Rune `on_press` handler would freeze the UI for seconds. The
shell therefore renders on a background worker thread: UI handlers only queue a
`NavRequest` (instant), the worker owns its own `WebView`, and `State::poll`
drains finished `NavResult`s into `rune::Pixels` each frame. Navigation requests
carry a generation number, so a burst of rapid clicks settles on the newest
page instead of flickering through stale results. `back`/`forward`/`reload` run
on the worker's own history, staying in sync with what was rendered.

Each tab has an independent `WebView` on the worker (keyed by a never-reused id),
so results from a closed tab are discarded and cannot repaint a different tab.
The start page's RGBA bytes are rendered once and cached, so opening a tab just
copies bytes into a fresh `Pixels` handle — no `WebView` is created on the UI
thread. `Tab.loading` flips true on request and false when the newest result is
applied, driving the progress line.

## Layout

The view recreates the reference Arc-style browser chrome:

- top toolbar: traffic lights, sidebar toggle, back/forward/reload, a centred
  capsule address bar with a lock icon, and settings/share/new-tab/menu buttons;
- tab strip: one tab per page with its own engine `WebView` and history; click a
  tab to switch instantly (cached pixels, no engine work), `+` opens a new tab,
  `×` closes one. Capped at `MAX_TABS` (12). A 2px accent line under the strip
  shows a load in progress while the worker renders off-thread;
- left rail: icon strip with spaces, folders, share, add at the top and
  settings, books, help, avatar at the bottom;
- left panel: **Espacios** and **Favoritos** grids plus a bookmark list, all
  driven by state and scrollable;
- centre pane: the Megi-rendered page, with an error line when navigation fails
  and a transient toast giving feedback for share/settings/menu/rail actions
  (timed on Rune's frame clock and cleared in `poll`).
  The address bar follows every navigation (and its final URL after redirects);
  `data:` documents are shown under a friendly label instead of a raw blob, and
  each tab's title is derived from the site it visited so tabs stay distinct.

Icons are vector paths (`Shape::fill_svg` / `stroke_svg`); colours are authored
in OKLCH to match the dark, rounded reference chrome. Interactive controls use
`.hover_fill()` and Rune's `motion::SNAPPY` spring.

### Responsive behaviour

The window size is read from an `on_layout` callback on the root element and
drives a breakpoint (520 pt):

- **wide windows** — full toolbar, rail + wide sidebar panel, page scaled with
  `SurfaceFit::Fit`;
- **narrow windows** — the wide panel collapses to the icon rail and the four
  trailing toolbar actions are dropped (they remain in the rail), so the page
  keeps a usable share of the window.

The page uses `pixels_fit(..., Fit)` so resizing letterboxes the engine frame
instead of stretching the article. Space tiles use a transparent `outline` on
inactive tiles so switching spaces never shifts the row's layout.

### Stress tests

`crates/shell/src/stress.rs` hammers the UI the way a user would: every toolbar
control pressed repeatedly, sidebar toggled mid-click, 12 viewport sizes from
1×1 to 4K/ultrawide, resize while the sidebar flips, rapid bookmark/space
navigation, wheel scrolling, garbage URL input, tab create/switch/close hammering,
a stale-result isolation check, loading-state lifecycle, Unicode titles, and
timing assertions that clicks and tab switches stay non-blocking. Headless
`Runtime` tests register the same `poll` callback the real app uses, so worker
results are actually applied, toast show/expire lifecycle, UTF-8 tab titles,
address-bar synchronisation on navigate/redirect, and per-site tab titles.
Full suite: 21 tests in ~2.4 s (down from ~174 s before navigation became
asynchronous).

## Build & run

```sh
cargo check -p megi-rune-shell
cargo test -p megi-rune-shell
cargo run -p megi-rune-shell                       # with a display
cargo run -p megi-rune-shell -- --snap shell.png   # headless snapshot
cargo run -p megi-rune-shell -- --tree             # dump element tree
cargo run -p megi-rune-shell -- --snap x.png --scene no-sidebar
cargo run -p megi-rune-shell -- --snap x.png --scene inline-page --settle
cargo run -p megi-rune-shell -- --snap x.png --size 500x400 --settle  # narrow window
```

Rune ships vendored `[patch.crates-io]` entries that Cargo does **not** inherit
from a dependency, so they are repeated at this workspace root, pinned to the
same rev as the `rune` dependency. A `.cargo/config.toml` enables
`git-fetch-with-cli` for reliable fetching of the git dependencies.

## Dependencies

- `rune` — git rev `4918969f04dce020f63b34e024368a86e5e1857d`
- `megi-webview` — git rev `b8d5e3d2f9f97eecd768bfb4e0113b4d312c9138`

## Status

Prototype. Navigation to real `https://` URLs depends on Megi's network loader;
the default view is an offline, self-contained Spanish Wikipedia-style page so
headless snapshots are deterministic. Clicking favourites/bookmarks triggers a
real `WebView::load()`; a subsequent frame picks up the new pixels.
