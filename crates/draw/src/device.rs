//! Drawing device abstraction and SVG backend.
//!
//! [`DrawDevice`] is the abstract trait corresponding to the C++ `device` abstract class.
//! [`SvgDevice`] implements it by emitting SVG 1.1 XML to a [`std::io::Write`] sink.
//!
//! C++ references:
//! - `device/device.h` — abstract `device` class with 11 pure-virtual methods.
//! - `device/SVGDev.h` / `device/SVGDev.cpp` — concrete SVG backend.

use std::io::{BufWriter, Write};

use crate::DrawConfig;
use crate::error::DrawError;

// ─── DrawDevice trait ──────────────────────────────────────────────────────────

/// Abstract drawing device with 11 primitive operations.
///
/// Each method corresponds directly to a pure-virtual method on C++ `device`.
pub trait DrawDevice {
    /// Filled rectangle with a drop-shadow, optional hyperlink.
    ///
    /// C++ reference: `SVGDev.cpp:131` — `SVGDev::rect`
    fn rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: &str,
        link: &str,
    ) -> Result<(), DrawError>;

    /// Filled triangle (merge / split symbol) with a small circle at the tip.
    ///
    /// C++ reference: `SVGDev.cpp:166` — `SVGDev::triangle`
    fn triangle(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: &str,
        link: &str,
        left_right: bool,
    ) -> Result<(), DrawError>;

    /// Filled circle (`rond`).
    ///
    /// C++ reference: `SVGDev.cpp:196` — `SVGDev::rond`
    fn circle(&mut self, x: f64, y: f64, radius: f64) -> Result<(), DrawError>;

    /// Square outline centered at `(x, y)` with side `side` (`carre`).
    ///
    /// C++ reference: `SVGDev.cpp:232` — `SVGDev::carre`
    fn square(&mut self, x: f64, y: f64, side: f64) -> Result<(), DrawError>;

    /// Two-line arrowhead at `(x, y)` with `rotation` degrees, pointing in `direction`.
    ///
    /// C++ reference: `SVGDev.cpp:201` — `SVGDev::fleche`
    fn arrow(&mut self, x: f64, y: f64, rotation: f64, direction: i32) -> Result<(), DrawError>;

    /// Solid wire segment.
    ///
    /// C++ reference: `SVGDev.cpp:240` — `SVGDev::trait`
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError>;

    /// Dashed wire segment (used for group borders).
    ///
    /// C++ reference: `SVGDev.cpp:249` — `SVGDev::dasharray`
    fn dashed_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError>;

    /// Centered white text with optional hyperlink.
    ///
    /// C++ reference: `SVGDev.cpp:258` — `SVGDev::text`
    fn text(&mut self, x: f64, y: f64, name: &str, link: &str) -> Result<(), DrawError>;

    /// Left-aligned dark text label (group names, annotations).
    ///
    /// C++ reference: `SVGDev.cpp:276` — `SVGDev::label`
    fn label(&mut self, x: f64, y: f64, name: &str) -> Result<(), DrawError>;

    /// Small dot indicating port orientation (`markSens`).
    ///
    /// C++ reference: `SVGDev.cpp:283` — `SVGDev::markSens`
    fn mark_direction(&mut self, x: f64, y: f64, direction: i32) -> Result<(), DrawError>;

    /// Error annotation displayed in red.
    ///
    /// C++ reference: `SVGDev.cpp:289` — `SVGDev::Error`
    fn error_msg(
        &mut self,
        msg: &str,
        reason: &str,
        n: usize,
        x: f64,
        y: f64,
        w: f64,
    ) -> Result<(), DrawError>;
}

// ─── XML escaping ──────────────────────────────────────────────────────────────

/// Escape special XML characters in `s`.
///
/// C++ reference: `SVGDev.cpp:34` — `static char* xmlcode(const char* name, char* name2)`.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            other => out.push(other),
        }
    }
    out
}

// ─── SvgDevice ─────────────────────────────────────────────────────────────────

/// SVG 1.1 drawing backend.
///
/// Opens a file, writes the `<svg>` header with `viewBox` in the constructor,
/// and closes `</svg>` in [`Drop`] (or explicitly via [`SvgDevice::finish`]).
/// All draw calls emit raw XML using `write!()`.
///
/// Optional features controlled by [`DrawConfig`]:
/// - `shadow_blur`: emits a `<defs><filter>` Gaussian blur and applies it to shadow rects.
/// - `scaled_svg`: omits the fixed `width=`/`height=` mm attributes for a responsive viewBox.
///
/// C++ reference: `device/SVGDev.h` / `device/SVGDev.cpp`.
pub struct SvgDevice<W: Write> {
    // Option so that finish() can take ownership without Drop writing a second </svg>.
    writer: Option<BufWriter<W>>,
    shadow_blur: bool,
}

impl<W: Write> SvgDevice<W> {
    fn w(&mut self) -> &mut BufWriter<W> {
        self.writer.as_mut().expect("SvgDevice already finished")
    }

    /// Create a new SVG device writing to `writer`.
    ///
    /// Emits the `<?xml?>` declaration and `<svg>` tag.
    /// - Without `config.scaled_svg`: includes `width=`/`height=` in mm at 0.5× scale.
    /// - With `config.scaled_svg`: viewBox only — responsive, no fixed dimensions.
    /// - With `config.shadow_blur`: emits a `<defs>` Gaussian blur filter.
    ///
    /// C++ reference: `SVGDev.cpp:86` — `SVGDev::SVGDev`.
    pub fn new(writer: W, width: f64, height: f64, config: &DrawConfig) -> Result<Self, DrawError> {
        let scale = 0.5_f64;
        let mut bw = BufWriter::new(writer);
        writeln!(bw, r#"<?xml version="1.0"?>"#)?;
        if config.scaled_svg {
            writeln!(
                bw,
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 {width} {height}" version="1.1">"#,
            )?;
        } else {
            writeln!(
                bw,
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 {width} {height}" width="{wmm}mm" height="{hmm}mm" version="1.1">"#,
                wmm = width * scale,
                hmm = height * scale,
            )?;
        }
        // Shadow-blur filter definition.
        // C++ reference: SVGDev.cpp — `gShadowBlur` block in constructor.
        if config.shadow_blur {
            writeln!(bw, "<defs>")?;
            writeln!(bw, r#"  <filter id="filter" filterRes="18" x="0" y="0">"#)?;
            writeln!(bw, r#"    <feGaussianBlur in="SourceGraphic" stdDeviation="1.55" result="blur"/>"#)?;
            writeln!(bw, r#"    <feOffset in="blur" dx="3" dy="3"/>"#)?;
            writeln!(bw, "  </filter>")?;
            writeln!(bw, "</defs>")?;
        }
        Ok(Self { writer: Some(bw), shadow_blur: config.shadow_blur })
    }

    /// Flush, write the closing `</svg>` tag, and return the inner writer.
    ///
    /// Called automatically by [`Drop`]; can also be called explicitly.
    pub fn finish(mut self) -> Result<W, DrawError> {
        let mut bw = self.writer.take().expect("already finished");
        writeln!(bw, "</svg>")?;
        bw.flush()?;
        Ok(bw.into_inner().map_err(|e| DrawError::Io(e.into_error()))?)
    }
}

impl<W: Write> Drop for SvgDevice<W> {
    fn drop(&mut self) {
        if let Some(ref mut bw) = self.writer {
            let _ = writeln!(bw, "</svg>");
            let _ = bw.flush();
        }
    }
}

impl<W: Write> DrawDevice for SvgDevice<W> {
    /// C++ reference: `SVGDev.cpp:131`
    fn rect(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: &str,
        link: &str,
    ) -> Result<(), DrawError> {
        if !link.is_empty() {
            writeln!(self.w(), r#"<a xlink:href="{}">"#, xml_escape(link))?;
        }
        // shadow rectangle — blur filter when enabled, plain grey otherwise
        // C++ reference: SVGDev.cpp — gShadowBlur branch
        if self.shadow_blur {
            writeln!(
                self.w(),
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="0" ry="0" style="stroke:none;fill:#aaaaaa;filter:url(#filter);"/>"#,
                x + 1.0, y + 1.0, w, h
            )?;
        } else {
            writeln!(
                self.w(),
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="0" ry="0" style="stroke:none;fill:#cccccc;"/>"#,
                x + 1.0, y + 1.0, w, h
            )?;
        }
        // colored rectangle
        writeln!(
            self.w(),
            r#"<rect x="{}" y="{}" width="{}" height="{}" rx="0" ry="0" style="stroke:none;fill:{};"/>"#,
            x, y, w, h, color
        )?;
        if !link.is_empty() {
            writeln!(self.w(), "</a>")?;
        }
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:166`
    fn triangle(
        &mut self,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: &str,
        link: &str,
        left_right: bool,
    ) -> Result<(), DrawError> {
        if !link.is_empty() {
            writeln!(self.w(), r#"<a xlink:href="{}">"#, xml_escape(link))?;
        }
        let r = 1.5_f64;
        let (x0, x1, x2) = if left_right {
            (x, x + w - 2.0 * r, x + w - r)
        } else {
            (x + w, x + 2.0 * r, x + r)
        };
        writeln!(
            self.w(),
            r#"<polygon fill="{color}" stroke="black" stroke-width=".25" points="{x0},{y} {x1},{mid} {x0},{bot}"/>"#,
            mid = y + h / 2.0,
            bot = y + h,
        )?;
        writeln!(
            self.w(),
            r#"<circle fill="{color}" stroke="black" stroke-width=".25" cx="{x2}" cy="{mid}" r="{r}"/>"#,
            mid = y + h / 2.0,
        )?;
        if !link.is_empty() {
            writeln!(self.w(), "</a>")?;
        }
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:196`
    fn circle(&mut self, x: f64, y: f64, radius: f64) -> Result<(), DrawError> {
        writeln!(self.w(), r#"<circle cx="{x}" cy="{y}" r="{radius}"/>"#)?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:232`
    fn square(&mut self, x: f64, y: f64, side: f64) -> Result<(), DrawError> {
        writeln!(
            self.w(),
            r#"<rect x="{}" y="{}" width="{side}" height="{side}" style="stroke:black;stroke-width:0.5;fill:none;"/>"#,
            x - 0.5 * side,
            y - side,
        )?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:201`
    fn arrow(&mut self, x: f64, y: f64, rotation: f64, direction: i32) -> Result<(), DrawError> {
        let (dx, dy) = (3.0_f64, 1.0_f64);
        let style = "stroke:black;stroke-width:0.25;";
        if direction == 1 {
            writeln!(
                self.w(),
                r#"<line x1="{}" y1="{}" x2="{x}" y2="{y}" transform="rotate({rotation},{x},{y})" style="{style}"/>"#,
                x - dx, y - dy,
            )?;
            writeln!(
                self.w(),
                r#"<line x1="{}" y1="{}" x2="{x}" y2="{y}" transform="rotate({rotation},{x},{y})" style="{style}"/>"#,
                x - dx, y + dy,
            )?;
        } else {
            writeln!(
                self.w(),
                r#"<line x1="{}" y1="{}" x2="{x}" y2="{y}" transform="rotate({rotation},{x},{y})" style="{style}"/>"#,
                x + dx, y - dy,
            )?;
            writeln!(
                self.w(),
                r#"<line x1="{}" y1="{}" x2="{x}" y2="{y}" transform="rotate({rotation},{x},{y})" style="{style}"/>"#,
                x + dx, y + dy,
            )?;
        }
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:240`
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError> {
        writeln!(
            self.w(),
            r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" style="stroke:black;stroke-linecap:round;stroke-width:0.25;"/>"#
        )?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:249`
    fn dashed_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) -> Result<(), DrawError> {
        writeln!(
            self.w(),
            r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" style="stroke:black;stroke-linecap:round;stroke-width:0.25;stroke-dasharray:3,3;"/>"#
        )?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:258`
    fn text(&mut self, x: f64, y: f64, name: &str, link: &str) -> Result<(), DrawError> {
        if !link.is_empty() {
            writeln!(self.w(), r#"<a xlink:href="{}">"#, xml_escape(link))?;
        }
        let escaped = xml_escape(name);
        let y2 = y + 2.0;
        writeln!(
            self.w(),
            r##"<text x="{x}" y="{y2}" font-family="Arial" font-size="7" text-anchor="middle" fill="#FFFFFF">{escaped}</text>"##,
        )?;
        if !link.is_empty() {
            writeln!(self.w(), "</a>")?;
        }
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:276`
    fn label(&mut self, x: f64, y: f64, name: &str) -> Result<(), DrawError> {
        let escaped = xml_escape(name);
        writeln!(
            self.w(),
            r#"<text x="{x}" y="{y2}" font-family="Arial" font-size="7">{escaped}</text>"#,
            y2 = y + 2.0,
        )?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:283`
    fn mark_direction(&mut self, x: f64, y: f64, direction: i32) -> Result<(), DrawError> {
        let offset = if direction == 1 { 2.0_f64 } else { -2.0_f64 };
        writeln!(self.w(), r#"<circle cx="{}" cy="{}" r="1"/>"#, x + offset, y + offset)?;
        Ok(())
    }

    /// C++ reference: `SVGDev.cpp:289`
    fn error_msg(
        &mut self,
        msg: &str,
        reason: &str,
        n: usize,
        x: f64,
        y: f64,
        w: f64,
    ) -> Result<(), DrawError> {
        let red = "stroke:red;stroke-width:0.3;fill:red;text-anchor:middle;";
        let emsg = xml_escape(msg);
        let ereason = xml_escape(reason);
        let y1 = y - 7.0;
        let y2 = y + 7.0;
        writeln!(
            self.w(),
            r#"<text x="{x}" y="{y1}" textLength="{w}" lengthAdjust="spacingAndGlyphs" style="{red}">{n} : {emsg}</text>"#,
        )?;
        writeln!(
            self.w(),
            r#"<text x="{x}" y="{y2}" textLength="{w}" lengthAdjust="spacingAndGlyphs" style="stroke:red;stroke-width:0.3;fill:none;text-anchor:middle;">{ereason}</text>"#,
        )?;
        Ok(())
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dev(width: f64, height: f64) -> SvgDevice<Vec<u8>> {
        SvgDevice::new(Vec::new(), width, height, &crate::DrawConfig::default()).unwrap()
    }

    fn svg_output(dev: SvgDevice<Vec<u8>>) -> String {
        let bytes = dev.finish().unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn test_svg_header_contains_viewbox() {
        let dev = make_dev(100.0, 50.0);
        let out = svg_output(dev);
        assert!(out.contains(r#"viewBox="0 0 100 50""#), "missing viewBox in:\n{out}");
        assert!(out.contains("<svg"), "missing <svg tag");
    }

    #[test]
    fn test_entity_escape_amp() {
        assert_eq!(xml_escape("A & B"), "A &amp; B");
    }

    #[test]
    fn test_entity_escape_lt_gt() {
        assert_eq!(xml_escape("a<b>c"), "a&lt;b&gt;c");
    }

    #[test]
    fn test_entity_escape_quotes() {
        assert_eq!(xml_escape(r#"say "hello""#), "say &quot;hello&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
    }

    #[test]
    fn test_line_emitted() {
        let mut dev = make_dev(100.0, 100.0);
        dev.line(0.0, 0.0, 10.0, 20.0).unwrap();
        let out = svg_output(dev);
        assert!(out.contains(r#"<line x1="0" y1="0" x2="10" y2="20""#), "missing line:\n{out}");
    }

    #[test]
    fn test_text_entity_in_label() {
        let mut dev = make_dev(100.0, 100.0);
        dev.label(5.0, 5.0, "A & B").unwrap();
        let out = svg_output(dev);
        assert!(out.contains("A &amp; B"), "entity not escaped:\n{out}");
    }
}
