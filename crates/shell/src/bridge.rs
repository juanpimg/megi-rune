//! Bridge between Megi's rendered frame and Rune's GPU image primitive.
//!
//! Megi's embeddable `WebView` renders a page into an owned RGBA8 `Pixmap`.
//! Rune paints an `el().pixels(&Pixels)` through its GPU renderer. The only
//! bridge needed is a byte copy: no FFI, no unsafe, shared `Vec<u8>` ownership.

use megi_webview::WebFrame;
use rune::Pixels;

/// Copies a Megi frame's row-major RGBA8 pixmap into a Rune `Pixels` handle.
///
/// `Pixels::set` keeps its own `Arc`'d copy of the bytes, so the frame can be
/// dropped after this call. Call `set` again on the same handle to update a
/// page without allocating a new GPU texture slot in the element tree.
pub fn frame_to_pixels(frame: &WebFrame) -> Pixels {
    let pixels = Pixels::new();
    update_pixels(&pixels, frame);
    pixels
}

/// Updates an existing `Pixels` handle in place with a freshly rendered frame.
pub fn update_pixels(pixels: &Pixels, frame: &WebFrame) {
    let w = frame.pixmap.width;
    let h = frame.pixmap.height;
    pixels.set(w, h, frame.pixmap.to_rgba_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use megi_webview::{WebView, WebViewOptions};

    #[test]
    fn a_loaded_page_produces_rgba_bytes_for_rune() {
        let opts = WebViewOptions::new(320, 200).unwrap();
        let mut view = WebView::new(opts).unwrap();
        let frame = view.load("data:text/html,<h1>Hola</h1>").unwrap();

        let px = frame_to_pixels(frame);
        let (w, h, rgba) = px.to_rgba().expect("a loaded frame has pixels");
        assert_eq!((w, h), (320, 200));
        assert_eq!(rgba.len(), 320 * 200 * 4);
    }
}
