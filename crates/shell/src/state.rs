//! Application state: the embedded Megi webview plus the Arc-style shell data.

use megi_webview::net::base64_encode;
use megi_webview::{WebView, WebViewOptions};
use rune::TextEdit;

use crate::bridge::frame_to_pixels;

/// Width/height of the engine viewport. Content is later scaled into the
/// shell's centre pane by Rune.
pub const PAGE_W: u32 = 1120;
pub const PAGE_H: u32 = 820;

/// One bookmark/favourite tile in the sidebar.
pub struct Favourite {
    pub label: &'static str,
    pub url: &'static str,
    pub glyph: &'static str,
    pub tint: u32,
}

/// A space (Arc-style workspace) with an accent colour.
pub struct Space {
    pub label: &'static str,
    pub tint: u32,
}

/// The whole UI state, owned by `rune::run` and handed to `view`.
pub struct State {
    pub web: WebView,
    pub page: rune::Pixels,
    pub url_edit: TextEdit,
    pub loaded_url: String,
    pub sidebar_open: bool,
    pub nav_error: Option<String>,
    pub spaces: Vec<Space>,
    pub active_space: usize,
    pub favourites: Vec<Favourite>,
}

/// HTML shown before any real page is loaded. Styled to look like the
/// Wikipedia article in the reference screenshot, fully offline so headless
/// snapshots are deterministic.
pub const START_HTML: &str = include_str!("start.html");

/// Wraps the built-in HTML in a base64 `data:` URL so `#`, `,`, quotes and
/// Unicode in the document cannot break `load()`'s URL parsing.
fn start_url() -> String {
    format!(
        "data:text/html;base64,{}",
        base64_encode(START_HTML.as_bytes())
    )
}

impl Default for State {
    fn default() -> Self {
        let opts = WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
        let mut web = WebView::new(opts).expect("engine initialises");
        let start = start_url();
        let frame = web
            .load(&start)
            .expect("the built-in start page always renders");
        let page = frame_to_pixels(frame);
        let loaded_url = frame.url.clone();

        let spaces = vec![
            Space {
                label: "Personal",
                tint: 0x8b7cf6,
            },
            Space {
                label: "Work",
                tint: 0x4f9cf9,
            },
            Space {
                label: "Study",
                tint: 0x32b67a,
            },
            Space {
                label: "Read",
                tint: 0xe8913a,
            },
        ];
        let favourites = vec![
            Favourite {
                label: "GitHub",
                url: "https://github.com",
                glyph: "GH",
                tint: 0x24292e,
            },
            Favourite {
                label: "YouTube",
                url: "https://youtube.com",
                glyph: "YT",
                tint: 0xff0033,
            },
            Favourite {
                label: "Wikipedia",
                url: "https://es.wikipedia.org",
                glyph: "W",
                tint: 0x2b2b2b,
            },
            Favourite {
                label: "Google",
                url: "https://google.com",
                glyph: "G",
                tint: 0x4285f4,
            },
            Favourite {
                label: "Reddit",
                url: "https://reddit.com",
                glyph: "r",
                tint: 0xff4500,
            },
            Favourite {
                label: "Kotlin",
                url: "https://kotlinlang.org",
                glyph: "K",
                tint: 0x7f52ff,
            },
            Favourite {
                label: "Figma",
                url: "https://figma.com",
                glyph: "F",
                tint: 0xf24e1e,
            },
            Favourite {
                label: "Docs",
                url: "https://developer.mozilla.org",
                glyph: "MD",
                tint: 0x000000,
            },
        ];

        Self {
            web,
            page,
            url_edit: TextEdit::new("https://es.wikipedia.org"),
            loaded_url,
            sidebar_open: true,
            nav_error: None,
            spaces,
            active_space: 0,
            favourites,
        }
    }
}

impl State {
    /// Navigates to `url`. Empty input restores the built-in start page.
    pub fn navigate(&mut self, url: &str) {
        let owned;
        let target = if url.trim().is_empty() {
            owned = start_url();
            owned.as_str()
        } else {
            url
        };
        match self.web.load(target) {
            Ok(frame) => {
                crate::bridge::update_pixels(&self.page, frame);
                self.loaded_url = frame.url.clone();
                self.nav_error = None;
            }
            Err(e) => {
                self.nav_error = Some(format!("{e:?}"));
            }
        }
    }

    pub fn back(&mut self) {
        if let Some(frame) = self.web.back() {
            crate::bridge::update_pixels(&self.page, frame);
            self.loaded_url = frame.url.clone();
            self.nav_error = None;
        }
    }

    pub fn forward(&mut self) {
        if let Some(frame) = self.web.forward() {
            crate::bridge::update_pixels(&self.page, frame);
            self.loaded_url = frame.url.clone();
            self.nav_error = None;
        }
    }

    pub fn reload(&mut self) {
        if let Some(frame) = self.web.reload() {
            crate::bridge::update_pixels(&self.page, frame);
            self.loaded_url = frame.url.clone();
            self.nav_error = None;
        }
    }

    pub fn commit_url_bar(&mut self) {
        let typed = self.url_edit.text.clone();
        self.navigate(&typed);
    }
}
