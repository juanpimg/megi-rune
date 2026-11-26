//! Application state: the embedded Megi webview plus the Arc-style shell data.
//!
//! Navigation is asynchronous: Megi's `WebView::load` renders synchronously
//! (and can block on the network), so doing it inside a Rune `on_press` handler
//! freezes the UI thread for seconds. Instead, a background worker owns its own
//! `WebView`, renders there, and posts the resulting pixels back through a
//! channel. `State::poll` drains finished navigations and repaints — the UI
//! thread never blocks.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use megi_webview::net::base64_encode;
use megi_webview::{WebView, WebViewOptions};
use rune::{TextEdit, Tx};

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

/// What the worker should render. Back/forward/reload operate on the worker's
/// own WebView history, so they always stay in sync with what it rendered.
#[derive(Clone)]
enum NavTarget {
    Url(String),
    Back,
    Forward,
    Reload,
}

/// A request posted to the render worker, tagged so stale results can be
/// discarded when the user navigates several times quickly.
#[derive(Clone)]
struct NavRequest {
    target: NavTarget,
    generation: u64,
}

/// A finished navigation posted back from the render worker.
struct NavResult {
    generation: u64,
    url: Option<String>,
    rgba: Option<Vec<u8>>,
    width: u32,
    height: u32,
    error: Option<String>,
}

/// A cloneable handle the view's `on_press` closures use to request navigation.
/// `Arc<Mutex<_>>` lets `'static` closures share the channel sender.
#[derive(Clone)]
pub struct NavSender {
    tx: Arc<Mutex<Sender<NavRequest>>>,
    generation: Arc<Mutex<u64>>,
}

impl NavSender {
    fn send(&self, target: NavTarget) {
        let mut counter = self.generation.lock().expect("generation mutex poisoned");
        *counter += 1;
        let request = NavRequest {
            target,
            generation: *counter,
        };
        drop(counter);
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(request);
        }
    }

    /// Queues a URL for rendering. Non-blocking: never touches the engine here.
    pub fn request(&self, url: &str) {
        self.send(NavTarget::Url(url.to_owned()));
    }
    pub fn back(&self) {
        self.send(NavTarget::Back);
    }
    pub fn forward(&self) {
        self.send(NavTarget::Forward);
    }
    pub fn reload(&self) {
        self.send(NavTarget::Reload);
    }
}

/// Spawns the background render worker. It owns a `WebView` with the same
/// viewport and renders requests one at a time, posting each result back.
fn spawn_worker(results: Sender<NavResult>, requests: Receiver<NavRequest>) {
    thread::Builder::new()
        .name("megi-render".to_owned())
        .spawn(move || {
            let opts = WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
            let mut web = WebView::new(opts).expect("engine initialises");
            // Prime the worker's URL history so back/forward work there too.
            let start = start_url();
            let _ = web.load(&start);

            while let Ok(req) = requests.recv() {
                let mut result = NavResult {
                    generation: req.generation,
                    url: None,
                    rgba: None,
                    width: PAGE_W,
                    height: PAGE_H,
                    error: None,
                };
                // Normalise every outcome to Option<&WebFrame> + optional error.
                // `load` returns Result; history moves return Option (None when
                // there is no such entry — that is not an error, just a no-op).
                let (frame, error): (Option<&megi_webview::WebFrame>, Option<String>) =
                    match &req.target {
                        NavTarget::Url(url) => match web.load(url) {
                            Ok(f) => (Some(f), None),
                            Err(e) => (None, Some(format!("{e}"))),
                        },
                        NavTarget::Back => (web.back(), None),
                        NavTarget::Forward => (web.forward(), None),
                        NavTarget::Reload => (web.reload(), None),
                    };
                match frame {
                    Some(f) => {
                        result.url = Some(f.url.clone());
                        result.width = f.pixmap.width;
                        result.height = f.pixmap.height;
                        result.rgba = Some(f.pixmap.to_rgba_bytes());
                    }
                    None => {
                        if let Some(e) = error {
                            result.error = Some(e);
                        }
                    }
                }
                // A closed receiver means the app is gone; stop quietly.
                if results.send(result).is_err() {
                    break;
                }
            }
        })
        .expect("render worker thread spawns");
}

/// The whole UI state, owned by `rune::run` and handed to `view`.
pub struct State {
    pub page: rune::Pixels,
    pub url_edit: TextEdit,
    pub loaded_url: String,
    pub sidebar_open: bool,
    pub nav_error: Option<String>,
    pub spaces: Vec<Space>,
    pub active_space: usize,
    pub favourites: Vec<Favourite>,
    /// Cloneable, thread-safe handle used by UI handlers.
    pub nav: NavSender,
    /// Results from the render worker, drained by `poll`.
    results: Receiver<NavResult>,
    /// Generation of the most recently *applied* result; stale ones are dropped
    /// so rapid back/forward/click sequences cannot show an outdated page.
    applied_generation: u64,
    /// Latest window size in logical points, reported by `on_layout` on the
    /// root element. Used for responsive breakpoints (e.g. collapsing the
    /// wide sidebar on narrow windows).
    pub viewport_w: f32,
    pub viewport_h: f32,
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
        // Initial frame is rendered synchronously once, at startup, so the
        // first paint has content. Every later navigation uses the worker.
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

        let (req_tx, req_rx) = channel::<NavRequest>();
        let (res_tx, res_rx) = channel::<NavResult>();
        spawn_worker(res_tx, req_rx);

        Self {
            page,
            url_edit: TextEdit::new("https://es.wikipedia.org"),
            loaded_url,
            sidebar_open: true,
            nav_error: None,
            spaces,
            active_space: 0,
            favourites,
            nav: NavSender {
                tx: Arc::new(Mutex::new(req_tx)),
                generation: Arc::new(Mutex::new(0)),
            },
            results: res_rx,
            applied_generation: 0,
            viewport_w: 1280.0,
            viewport_h: 800.0,
        }
    }
}

impl State {
    /// Requests navigation without blocking the caller.
    pub fn navigate(&mut self, url: &str) {
        let target = if url.trim().is_empty() {
            start_url()
        } else {
            url.to_owned()
        };
        // Reflect the intent immediately in the URL bar and clear stale errors.
        self.nav_error = None;
        self.loaded_url = target.clone();
        self.nav.request(&target);
    }

    pub fn back(&mut self) {
        self.nav.back();
    }
    pub fn forward(&mut self) {
        self.nav.forward();
    }
    pub fn reload(&mut self) {
        self.nav.reload();
    }

    pub fn commit_url_bar(&mut self) {
        // Accept the raw text: load errors are reported through `nav_error`,
        // so an invalid URL must never panic or freeze the shell.
        let typed = self.url_edit.text.clone();
        self.navigate(&typed);
    }

    /// Applies any finished navigations. Called from Rune's `poll` each frame.
    /// Returns true when the page changed (so Rune re-runs the view).
    pub fn poll_navigation(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            // Drain the whole queue and keep only the newest generation: a
            // burst of rapid clicks settles on the last requested page.
            if result.generation < self.applied_generation {
                continue;
            }
            self.applied_generation = result.generation;
            if let Some(err) = &result.error {
                self.nav_error = Some(err.clone());
            } else {
                if let Some(rgba) = result.rgba {
                    self.page.set(result.width, result.height, rgba);
                }
                if let Some(url) = result.url {
                    self.loaded_url = url;
                }
                self.nav_error = None;
            }
            changed = true;
        }
        changed
    }

    /// Synchronously applies a navigation, for tests and headless snapshots
    /// that must see the result in the same frame.
    pub fn navigate_blocking(&mut self, url: &str) {
        let opts = WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
        let mut web = WebView::new(opts).expect("engine initialises");
        match web.load(url) {
            Ok(frame) => {
                self.page.set(
                    frame.pixmap.width,
                    frame.pixmap.height,
                    frame.pixmap.to_rgba_bytes(),
                );
                self.loaded_url = frame.url.clone();
                self.nav_error = None;
            }
            Err(e) => {
                self.nav_error = Some(format!("{e}"));
            }
        }
    }
}

/// Rune `poll` callback: drains worker results every frame.
pub fn poll(s: &mut State, _clock: f64, _tx: &mut Tx) -> bool {
    s.poll_navigation()
}
