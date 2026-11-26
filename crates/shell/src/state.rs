//! Application state: the embedded Megi webview plus the Arc-style shell data.
//!
//! Navigation is asynchronous: Megi's `WebView::load` renders synchronously
//! (and can block on the network), so doing it inside a Rune `on_press` handler
//! freezes the UI thread for seconds. Instead, a background worker owns one
//! `WebView` **per tab**, renders there, and posts the resulting pixels back
//! through a channel. Each tab keeps its own page and history, so switching
//! tabs is an instant pixel swap with no engine work on the UI thread.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use megi_webview::net::base64_encode;
use megi_webview::{WebView, WebViewOptions};
use rune::{Key, KeyEvent, TextEdit, Tx};

use crate::bridge::{frame_to_pixels, update_pixels};

/// Width/height of the engine viewport. Content is later scaled into the
/// shell's centre pane by Rune.
pub const PAGE_W: u32 = 1120;
pub const PAGE_H: u32 = 820;

/// Maximum number of tabs. Bounded so a click-happy user cannot grow the
/// worker's `Vec<WebView>` (and its frame buffers) without limit.
pub const MAX_TABS: usize = 12;

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

/// A clickable link region in CSS pixels, extracted from a rendered frame.
/// Kept as plain data so it crosses the worker → UI channel cheaply and lets the
/// UI hit-test a click without a round trip to the engine.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkRegion {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub href: String,
}

/// Collects every `<a href>` and its painted rectangles from a frame. Uses
/// Megi's own `find_link_at` (which walks DOM ancestors) so text runs inside a
/// link resolve to the anchor, then verifies the region actually belongs to
/// that anchor to avoid picking up an overlapping link at the region centre.
fn links_from_frame(frame: &megi_webview::WebFrame) -> Vec<LinkRegion> {
    let doc = &frame.document;
    let regions = &frame.hit_regions;
    let mut links = Vec::new();
    for region in regions {
        let Some(node) = region.node_id else { continue };
        if region.rect.width <= 0.0 || region.rect.height <= 0.0 {
            continue;
        }
        let cx = region.rect.x + region.rect.width * 0.5;
        let cy = region.rect.y + region.rect.height * 0.5;
        let Some(link) = megi_webview::display::find_link_at(doc, regions, cx, cy) else {
            continue;
        };
        // The <a> must be this region's node or one of its ancestors.
        let mut cur = Some(node);
        let mut belongs = false;
        while let Some(n) = cur {
            if n == link.node_id {
                belongs = true;
                break;
            }
            let parent = doc.node(n).parent;
            cur = parent.filter(|p| doc.node(*p).as_element().is_some());
        }
        if !belongs {
            continue;
        }
        let href = link.href.trim().to_owned();
        let lower = href.to_ascii_lowercase();
        // Anchors to the same document and script/mail links are not navigations.
        if href.is_empty()
            || href.starts_with('#')
            || lower.starts_with("javascript:")
            || lower.starts_with("mailto:")
        {
            continue;
        }
        links.push(LinkRegion {
            x: region.rect.x,
            y: region.rect.y,
            w: region.rect.width,
            h: region.rect.height,
            href,
        });
    }
    links
}

/// Resolves a raw `href` against the page's URL (the host's job per Megi's API).
pub fn resolve_href(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.is_empty() {
        return base.to_owned();
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return href.to_owned();
    }
    let Some(scheme_end) = base.find("://") else {
        return href.to_owned();
    };
    let scheme = &base[..scheme_end];
    let after = &base[scheme_end + 3..];
    let host_end = after.find('/').unwrap_or(after.len());
    let host = &after[..host_end];
    if let Some(rest) = href.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    if href.starts_with('/') {
        return format!("{scheme}://{host}{href}");
    }
    let mut segments: Vec<&str> = if host_end < after.len() {
        after[host_end + 1..].split('/').collect()
    } else {
        Vec::new()
    };
    if !segments.is_empty() {
        segments.pop(); // drop the document filename → directory
    }
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(seg),
        }
    }
    if segments.is_empty() {
        format!("{scheme}://{host}/")
    } else {
        format!("{scheme}://{host}/{}", segments.join("/"))
    }
}

/// A browser tab: only UI-side data lives here; the engine frame lives in the
/// worker and its last rendered pixels are cached in `page` for instant swaps.
pub struct Tab {
    pub id: u64,
    pub title: String,
    pub page: rune::Pixels,
    /// Native size of the cached page frame (for CSS↔layout coordinate mapping).
    pub page_w: u32,
    pub page_h: u32,
    /// Clickable link regions in CSS pixels, from the last rendered frame.
    pub links: Vec<LinkRegion>,
    pub url_edit: TextEdit,
    pub loaded_url: String,
    pub nav_error: Option<String>,
    /// True between a navigation request and its result being applied, so the
    /// view can show a spinner/dimming while the worker renders off-thread.
    pub loading: bool,
    /// Incremented on every request; results older than this are ignored.
    pub generation: u64,
}

/// What the worker should render. Back/forward/reload operate on the tab's own
/// WebView history, so they always stay in sync with what it rendered.
#[derive(Clone)]
enum NavTarget {
    Url(String),
    Back,
    Forward,
    Reload,
}

/// A request posted to the render worker.
#[derive(Clone)]
struct NavRequest {
    tab_id: u64,
    target: NavTarget,
    generation: u64,
}

/// A finished navigation posted back from the render worker.
struct NavResult {
    tab_id: u64,
    generation: u64,
    url: Option<String>,
    rgba: Option<Vec<u8>>,
    width: u32,
    height: u32,
    links: Vec<LinkRegion>,
    error: Option<String>,
}

/// A cloneable handle the view's `on_press` closures use to request navigation.
/// `Arc<Mutex<_>>` lets `'static` closures share the channel sender and the
/// per-tab generation counters.
#[derive(Clone)]
pub struct NavSender {
    tx: Arc<Mutex<Sender<NavRequest>>>,
    generations: Arc<Mutex<HashMap<u64, u64>>>,
}

impl NavSender {
    fn send(&self, tab_id: u64, target: NavTarget) {
        let generation = {
            let mut map = self.generations.lock().expect("generation mutex poisoned");
            let counter = map.entry(tab_id).or_insert(0);
            *counter += 1;
            *counter
        };
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(NavRequest {
                tab_id,
                target,
                generation,
            });
        }
    }

    /// Queues a URL for rendering. Non-blocking: never touches the engine here.
    pub fn request(&self, tab_id: u64, url: &str) {
        self.send(tab_id, NavTarget::Url(url.to_owned()));
    }
    pub fn back(&self, tab_id: u64) {
        self.send(tab_id, NavTarget::Back);
    }
    pub fn forward(&self, tab_id: u64) {
        self.send(tab_id, NavTarget::Forward);
    }
    pub fn reload(&self, tab_id: u64) {
        self.send(tab_id, NavTarget::Reload);
    }
}

/// Spawns the background render worker. It owns one `WebView` per tab, renders
/// requests one at a time, and posts each result back. Tabs are created lazily:
/// the worker keeps a `HashMap<u64, WebView>` and primes every new tab with the
/// built-in start page so history commands work immediately.
fn spawn_worker(results: Sender<NavResult>, requests: Receiver<NavRequest>) {
    thread::Builder::new()
        .name("megi-render".to_owned())
        .spawn(move || {
            let mut views: HashMap<u64, WebView> = HashMap::new();
            while let Ok(req) = requests.recv() {
                let web = views.entry(req.tab_id).or_insert_with(|| {
                    let opts =
                        WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
                    let mut web = WebView::new(opts).expect("engine initialises");
                    let _ = web.load(&start_url());
                    web
                });
                let mut result = NavResult {
                    tab_id: req.tab_id,
                    generation: req.generation,
                    url: None,
                    rgba: None,
                    width: PAGE_W,
                    height: PAGE_H,
                    links: Vec::new(),
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
                        result.links = links_from_frame(f);
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
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    /// Monotonic id source; ids are never reused, so a queued result for a
    /// closed tab cannot be applied to a newly opened one.
    next_tab_id: u64,
    pub sidebar_open: bool,
    pub spaces: Vec<Space>,
    pub active_space: usize,
    pub favourites: Vec<Favourite>,
    /// Cloneable, thread-safe handle used by UI handlers.
    pub nav: NavSender,
    /// Results from the render worker, drained by `poll`.
    results: Receiver<NavResult>,
    /// Latest window size in logical points, reported by `on_layout` on the
    /// root element. Used for responsive breakpoints (e.g. collapsing the
    /// wide sidebar on narrow windows).
    pub viewport_w: f32,
    pub viewport_h: f32,
    /// Laid-out rect `[x, y, w, h]` of the page element (reported by its
    /// `on_layout`), used to map a click back into the frame's CSS coordinates
    /// (replicating Rune's `pixels_fit(Fit)` letterbox).
    pub page_rect: [f32; 4],
    /// True while the pointer hovers a clickable link, so the view can show a
    /// pointing-hand cursor over the page.
    pub hover_link: bool,
    /// A short transient message shown as a toast for action feedback (share,
    /// settings, menu, etc.), with the frame clock deadline at which it expires.
    pub toast: Option<(String, f64)>,
}

impl State {
    /// Shows a toast until `now + seconds`. `now` comes from `Tx::clock()` in
    /// view handlers, so toasts expire on Rune's frame clock, not wall time.
    pub fn show_toast(&mut self, message: &str, now: f64, seconds: f64) {
        self.toast = Some((message.to_owned(), now + seconds));
    }

    /// Clears an expired toast. Called from `poll` every frame.
    pub fn expire_toast(&mut self, now: f64) -> bool {
        if self.toast.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.toast = None;
            return true;
        }
        false
    }

    /// Maps a click inside the page element (in the element's own layout
    /// points) into the frame's CSS-pixel coordinates, matching Rune's
    /// `pixels_fit(..., Fit)` placement (centred, aspect-preserving, letterboxed).
    /// Returns the resolved URL of the topmost link under the point, if any.
    pub fn link_at_local(&self, local_x: f32, local_y: f32) -> Option<String> {
        let tab = self.tab();
        let [_, _, nw, nh] = self.page_rect;
        let (sw, sh) = (tab.page_w as f32, tab.page_h as f32);
        if sw <= 0.0 || sh <= 0.0 || nw <= 0.0 || nh <= 0.0 {
            return None;
        }
        // Replicate image_rect(source, Fit, CENTRED): scale = min(nw/sw, nh/sh),
        // centred on both axes (share 0.5). `local_x/y` are already relative to
        // the element's own top-left, so we must NOT add the element's window
        // position (nx, ny) here — that would offset every click by the toolbar
        // and sidebar width in the real app.
        let scale = (nw / sw).min(nh / sh);
        let drawn_w = sw * scale;
        let drawn_h = sh * scale;
        let origin_x = (nw - drawn_w) * 0.5;
        let origin_y = (nh - drawn_h) * 0.5;
        let css_x = (local_x - origin_x) / scale;
        let css_y = (local_y - origin_y) / scale;
        // Links are painted in order; the last containing region wins, matching
        // Megi's own hit_test semantics (topmost paint = topmost link).
        tab.links
            .iter()
            .rev()
            .find(|l| css_x >= l.x && css_x < l.x + l.w && css_y >= l.y && css_y < l.y + l.h)
            .map(|l| resolve_href(&tab.loaded_url, &l.href))
    }
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

/// Turns an internal navigation URL into the short, human-friendly form shown
/// in the address bar. The start page's base64 `data:` blob would otherwise
/// flood the field; real `http(s)://` URLs are shown as-is (the lock icon in
/// the view conveys the scheme).
fn display_url(url: &str) -> String {
    if url.starts_with("data:text/html;base64,") {
        // The built-in start page: show the site it emulates, not its blob.
        "https://es.wikipedia.org".to_owned()
    } else if url.starts_with("data:") {
        // Any other inline document stays local; label it instead of flooding
        // the field with markup.
        "página local".to_owned()
    } else {
        url.to_owned()
    }
}

/// Derives a short tab title from a navigation URL. Known sites get a clean
/// name; anything else falls back to its hostname, so each tab is
/// distinguishable at a glance instead of all reading "Historia de España".
fn title_from_url(url: &str) -> String {
    if url.starts_with("data:") {
        return "Historia de España".to_owned();
    }
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .rsplit('@')
        .next()
        .unwrap_or(url)
        .split(':')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_owned();
    let known = [
        ("github.com", "GitHub"),
        ("youtube.com", "YouTube"),
        ("wikipedia.org", "Wikipedia"),
        ("google.com", "Google"),
        ("reddit.com", "Reddit"),
        ("kotlinlang.org", "Kotlin"),
        ("figma.com", "Figma"),
        ("developer.mozilla.org", "MDN Web Docs"),
    ];
    if let Some((_, name)) = known.iter().find(|(domain, _)| host.ends_with(domain)) {
        return (*name).to_owned();
    }
    if host.is_empty() {
        return "Nueva pestaña".to_owned();
    }
    host
}

/// Renders the built-in start page exactly once (lazily) and caches its raw
/// RGBA bytes. New tabs copy these bytes into their own `Pixels` handle, so
/// opening a tab is instant and never creates a `WebView` on the UI thread.
fn start_page_bytes() -> &'static (u32, u32, Vec<u8>) {
    static CACHE: OnceLock<(u32, u32, Vec<u8>)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let opts = WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
        let mut web = WebView::new(opts).expect("engine initialises");
        let frame = web
            .load(&start_url())
            .expect("the built-in start page always renders");
        let pixels = frame_to_pixels(frame);
        let (w, h, rgba) = pixels.to_rgba().expect("the cached start page has pixels");
        (w, h, rgba)
    })
}

/// Builds a tab showing the built-in start page, reusing the cached bytes.
fn initial_tab(id: u64) -> Tab {
    let (w, h, rgba) = start_page_bytes();
    let page = rune::Pixels::new();
    page.set(*w, *h, rgba.clone());
    Tab {
        id,
        title: "Historia de España".to_owned(),
        page,
        page_w: *w,
        page_h: *h,
        links: Vec::new(),
        url_edit: TextEdit::new("https://es.wikipedia.org"),
        loaded_url: start_url(),
        nav_error: None,
        loading: false,
        generation: 0,
    }
}

impl Default for State {
    fn default() -> Self {
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
            tabs: vec![initial_tab(1)],
            active_tab: 0,
            next_tab_id: 2,
            sidebar_open: true,
            spaces,
            active_space: 0,
            favourites,
            nav: NavSender {
                tx: Arc::new(Mutex::new(req_tx)),
                generations: Arc::new(Mutex::new(HashMap::new())),
            },
            results: res_rx,
            viewport_w: 1280.0,
            viewport_h: 800.0,
            page_rect: [0.0; 4],
            hover_link: false,
            toast: None,
        }
    }
}

impl State {
    /// Convenience access to the currently displayed tab.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }
    pub fn tab_mut(&mut self) -> &mut Tab {
        let i = self.active_tab;
        &mut self.tabs[i]
    }

    /// Opens a new tab with the built-in start page. Bounded by `MAX_TABS`.
    /// The initial pixels are rendered synchronously once so the swap is
    /// instant; all later navigation in that tab stays asynchronous.
    pub fn new_tab(&mut self) {
        if self.tabs.len() >= MAX_TABS {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(initial_tab(id));
        self.active_tab = self.tabs.len() - 1;
    }

    /// Opens a brand-new tab and immediately navigates it to `url`. Used by
    /// middle-click / Cmd-click on an in-page link. If the tab limit is hit it
    /// navigates the current tab instead, so the click is never swallowed.
    pub fn new_tab_with_url(&mut self, url: &str) {
        let before = self.tabs.len();
        self.new_tab();
        if self.tabs.len() == before {
            // At MAX_TABS: fall back to navigating the current tab.
            self.navigate(url);
            return;
        }
        self.navigate(url);
    }

    /// Closes a tab by id, keeping at least one tab open. Queued results for
    /// the closed id are discarded in `poll_navigation` (their tab no longer
    /// exists), so they cannot be applied to another tab.
    pub fn close_tab(&mut self, id: u64) {
        if self.tabs.len() <= 1 {
            return;
        }
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if self.active_tab > pos {
                self.active_tab -= 1;
            }
        }
    }

    /// Selects a tab by id. This only swaps which cached page is painted — no
    /// engine work happens on the UI thread, so switching is instant.
    pub fn select_tab(&mut self, id: u64) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.active_tab = pos;
        }
    }

    /// Requests navigation on the active tab without blocking the caller.
    pub fn navigate(&mut self, url: &str) {
        let tab = self.tab_mut();
        let target = if url.trim().is_empty() {
            start_url()
        } else {
            url.to_owned()
        };
        // Reflect the intent immediately in the URL bar and clear stale errors.
        tab.nav_error = None;
        tab.loaded_url = target.clone();
        tab.url_edit.replace_text(display_url(&target));
        tab.title = title_from_url(&target);
        tab.loading = true;
        let id = tab.id;
        self.nav.request(id, &target);
    }

    pub fn back(&mut self) {
        let id = self.tab().id;
        self.tab_mut().loading = true;
        self.nav.back(id);
    }
    pub fn forward(&mut self) {
        let id = self.tab().id;
        self.tab_mut().loading = true;
        self.nav.forward(id);
    }
    pub fn reload(&mut self) {
        let id = self.tab().id;
        self.tab_mut().loading = true;
        self.nav.reload(id);
    }

    pub fn commit_url_bar(&mut self) {
        // Accept the raw text: load errors are reported through `nav_error`,
        // so an invalid URL must never panic or freeze the shell.
        let typed = self.tab().url_edit.text.clone();
        self.navigate(&typed);
    }

    /// Applies any finished navigations. Called from Rune's `poll` each frame.
    /// Returns true when the page changed (so Rune re-runs the view).
    pub fn poll_navigation(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            // Locate the tab by id. Results for closed tabs are ignored.
            let Some(pos) = self.tabs.iter().position(|t| t.id == result.tab_id) else {
                continue;
            };
            let tab = &mut self.tabs[pos];
            // Drop stale results: a burst of rapid clicks settles on the last.
            if result.generation < tab.generation {
                continue;
            }
            tab.generation = result.generation;
            if let Some(err) = &result.error {
                tab.nav_error = Some(err.clone());
            } else {
                if let Some(rgba) = result.rgba {
                    tab.page.set(result.width, result.height, rgba);
                    tab.page_w = result.width;
                    tab.page_h = result.height;
                    tab.links = result.links.clone();
                }
                if let Some(url) = result.url {
                    tab.loaded_url = url.clone();
                    tab.title = title_from_url(&url);
                    // Keep the address bar in sync with the final URL (after
                    // redirects), but don't clobber what the user is typing if
                    // they started a new navigation while this one rendered.
                    if tab.loading {
                        tab.url_edit.replace_text(display_url(&url));
                    }
                }
                tab.nav_error = None;
            }
            // Only the newest result settles the loading state; older results
            // for the same tab were skipped above by the generation guard.
            tab.loading = false;
            changed = true;
        }
        changed
    }

    /// Synchronously applies a navigation on the active tab, for tests and
    /// headless snapshots that must see the result in the same frame.
    pub fn navigate_blocking(&mut self, url: &str) {
        let opts = WebViewOptions::new(PAGE_W, PAGE_H).expect("valid viewport dimensions");
        let mut web = WebView::new(opts).expect("engine initialises");
        match web.load(url) {
            Ok(frame) => {
                let tab = self.tab_mut();
                update_pixels(&tab.page, frame);
                tab.page_w = frame.pixmap.width;
                tab.page_h = frame.pixmap.height;
                tab.links = links_from_frame(frame);
                tab.loaded_url = frame.url.clone();
                tab.url_edit.replace_text(display_url(&frame.url));
                tab.title = title_from_url(&frame.url);
                tab.nav_error = None;
            }
            Err(e) => {
                self.tab_mut().nav_error = Some(format!("{e}"));
            }
        }
    }
}

/// Rune `poll` callback: drains worker results and expires toasts every frame.
pub fn poll(s: &mut State, clock: f64, _tx: &mut Tx) -> bool {
    let mut changed = s.poll_navigation();
    changed |= s.expire_toast(clock);
    changed
}

/// Browser-style keyboard shortcuts, handled at the app level for keys the
/// focused control did not consume. Returns true when a shortcut was handled.
///
/// - `Cmd/Ctrl+T` — new tab
/// - `Cmd/Ctrl+W` — close the active tab
/// - `Alt+←` / `Alt+→` — back / forward
/// - `Cmd/Ctrl+L` — focus the address bar (selecting its text)
/// - `F5` / `Cmd/Ctrl+R` — reload
pub fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    let mods = event.mods;
    // `Key::Command(c)` is Cmd+c on macOS and Ctrl+c elsewhere; `mods.primary()`
    // is true for both, so a single check covers both platforms.
    if let Key::Command(c) = &event.key
        && mods.primary()
    {
        match c.to_ascii_lowercase() {
            't' => {
                s.new_tab();
                return true;
            }
            'w' => {
                let id = s.tab().id;
                s.close_tab(id);
                return true;
            }
            'l' => {
                tx.focus("url-input", true);
                return true;
            }
            'r' => {
                s.reload();
                return true;
            }
            _ => {}
        }
    }
    match &event.key {
        Key::Left if mods.alt => {
            s.back();
            return true;
        }
        Key::Right if mods.alt => {
            s.forward();
            return true;
        }
        Key::Function(5) => {
            s.reload();
            return true;
        }
        _ => {}
    }
    false
}
