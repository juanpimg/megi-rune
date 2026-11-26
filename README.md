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
For a 1120×820 viewport that is ~3.7 MB per load; since Megi's `load()` is
synchronous and pages are static in this prototype, that cost is fine.

## Layout

The view recreates the reference Arc-style browser chrome:

- top toolbar: traffic lights, sidebar toggle, back/forward/reload, a centred
  capsule address bar with a lock icon, and settings/share/new-tab/menu buttons;
- left rail: icon strip with spaces, folders, share, add at the top and
  settings, books, help, avatar at the bottom;
- left panel: **Espacios** and **Favoritos** grids plus a bookmark list, all
  driven by state and scrollable;
- centre pane: the Megi-rendered page, with an error line when navigation fails.

Icons are vector paths (`Shape::fill_svg` / `stroke_svg`); colours are authored
in OKLCH to match the dark, rounded reference chrome. Interactive controls use
`.hover_fill()` and Rune's `motion::SNAPPY` spring.

## Build & run

```sh
cargo check -p megi-rune-shell
cargo test -p megi-rune-shell
cargo run -p megi-rune-shell                       # with a display
cargo run -p megi-rune-shell -- --snap shell.png   # headless snapshot
cargo run -p megi-rune-shell -- --tree             # dump element tree
cargo run -p megi-rune-shell -- --snap x.png --scene no-sidebar
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
