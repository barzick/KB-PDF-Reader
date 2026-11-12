//! Adding content on top of pages: text, watermarks, page numbers, headers
//! and footers. The original content is left untouched: new objects are
//! built on a scratch page and appended to the page's content stream.

use lopdf::Document;
use pdfium_render::prelude::*;
use serde::Deserialize;

use crate::edit::{load_font, StandardFamily, WriteFont};
use crate::{pdfium, KbError, Result};

type Rgb = [u8; 3];

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TextStyle {
    #[serde(default = "sans")]
    pub family: StandardFamily,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    pub size: f32,
    pub color: Rgb,
    /// 0 (invisible) to 1 (opaque).
    #[serde(default = "one")]
    pub opacity: f32,
}

fn sans() -> StandardFamily {
    StandardFamily::Sans
}
fn one() -> f32 {
    1.0
}

/// One line of text to draw. (u, v) is in displayed page coordinates, in
/// points from the bottom-left corner of the page as shown on screen.
#[derive(Debug, Clone)]
struct Item {
    text: String,
    style: TextStyle,
    u: f32,
    v: f32,
    align: Align,
    /// Rotation in degrees (counter-clockwise) around (u, v).
    angle: f32,
    /// Vertically centre the line on v (watermarks) instead of using v as baseline.
    middle: bool,
}

/// Where a page is, as displayed: its visible box and rotation.
#[derive(Debug, Clone, Copy)]
struct Geom {
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    rotate: i64,
}

impl Geom {
    /// Width and height as displayed.
    fn visual(&self) -> (f32, f32) {
        if self.rotate % 180 == 0 { (self.w, self.h) } else { (self.h, self.w) }
    }
}

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

fn geometry(doc: &Document, page_index: u32) -> Result<Geom> {
    let id = *doc.get_pages().get(&(page_index + 1)).ok_or(KbError::BadPage(page_index + 1))?;
    let inherited = |key: &[u8]| -> Option<lopdf::Object> {
        let mut d = doc.get_dictionary(id).ok()?;
        for _ in 0..32 {
            if let Ok(v) = d.get(key) {
                return Some(match v {
                    lopdf::Object::Reference(r) => doc.get_object(*r).ok()?.clone(),
                    v => v.clone(),
                });
            }
            d = doc.get_dictionary(d.get(b"Parent").ok()?.as_reference().ok()?).ok()?;
        }
        None
    };
    let rect = |o: Option<lopdf::Object>| -> Option<[f32; 4]> {
        let a = o?.as_array().ok()?.clone();
        let v: Vec<f32> = a.iter().filter_map(|n| n.as_float().ok()).collect();
        (v.len() == 4).then(|| [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])])
    };
    let media = rect(inherited(b"MediaBox")).unwrap_or([0.0, 0.0, 595.28, 841.89]);
    let crop = rect(inherited(b"CropBox")).unwrap_or(media);
    let rotate = inherited(b"Rotate").and_then(|o| o.as_i64().ok()).unwrap_or(0).rem_euclid(360);
    Ok(Geom { x0: crop[0], y0: crop[1], w: crop[2] - crop[0], h: crop[3] - crop[1], rotate })
}

/// Draws the items on top of one page.
fn draw(bytes: &[u8], page_index: u32, g: &Geom, items: &[Item]) -> Result<Vec<u8>> {
    if items.is_empty() {
        return Ok(bytes.to_vec());
    }
    let pdfium = pdfium()?;
    let mut scratch = pdfium.create_new_pdf()?;
    let mut fonts = vec![];
    for it in items {
        let s = it.style;
        fonts.push(load_font(&mut scratch, &WriteFont::Standard { bold: s.bold, italic: s.italic, family: s.family })?);
    }
    {
        let mut page = scratch
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(g.x0 + g.w), PdfPoints::new(g.y0 + g.h)))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        for (it, font) in items.iter().zip(fonts) {
            let s = it.style;
            let mut obj = page.objects_mut().create_text_object(
                PdfPoints::ZERO,
                PdfPoints::ZERO,
                &it.text,
                font,
                PdfPoints::new(s.size),
            )?;
            let alpha = (s.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
            obj.set_fill_color(PdfColor::new(s.color[0], s.color[1], s.color[2], alpha))?;
            let width = obj.bounds().map(|b| b.right().value - b.left().value).unwrap_or(0.0);
            let dx = match it.align {
                Align::Left => 0.0,
                Align::Center => -width / 2.0,
                Align::Right => -width,
            };
            let dy = if it.middle { -s.size * 0.35 } else { 0.0 };
            obj.translate(PdfPoints::new(dx), PdfPoints::new(dy))?;
            if it.angle != 0.0 {
                obj.rotate_counter_clockwise_degrees(it.angle)?;
            }
            obj.translate(PdfPoints::new(it.u), PdfPoints::new(it.v))?;
            // Displayed coordinates → page user space.
            let (tx, ty) = match g.rotate {
                90 => (g.x0 + g.w, g.y0),
                180 => (g.x0 + g.w, g.y0 + g.h),
                270 => (g.x0, g.y0 + g.h),
                _ => (g.x0, g.y0),
            };
            if g.rotate != 0 {
                obj.rotate_counter_clockwise_degrees(g.rotate as f32)?;
            }
            obj.translate(PdfPoints::new(tx), PdfPoints::new(ty))?;
        }
        page.regenerate_content()?;
    }
    let overlay = scratch.save_to_bytes()?;
    crate::surgery::remove_and_overlay(bytes, page_index, &[], 0, Some(&overlay))
}

fn page_count(doc: &Document) -> u32 {
    doc.get_pages().len() as u32
}

/// Pages to change: the given ones (0-based) or all.
fn targets(doc: &Document, pages: &Option<Vec<u32>>) -> Vec<u32> {
    let n = page_count(doc);
    match pages {
        Some(p) => p.iter().copied().filter(|&i| i < n).collect(),
        None => (0..n).collect(),
    }
}

fn load(bytes: &[u8]) -> Result<Document> {
    let doc = Document::load_mem(bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err(err("document protégé par mot de passe"));
    }
    Ok(doc)
}

/// Adds lines of text at a point of a page (top-left of the first line, in
/// displayed coordinates).
pub fn add_text(bytes: &[u8], page_index: u32, x: f32, y: f32, text: &str, style: TextStyle) -> Result<Vec<u8>> {
    let doc = load(bytes)?;
    let g = geometry(&doc, page_index)?;
    let items: Vec<Item> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(n, l)| Item {
            text: l.to_string(),
            style,
            u: x,
            v: y - style.size * (0.8 + n as f32 * 1.2),
            align: Align::Left,
            angle: 0.0,
            middle: false,
        })
        .collect();
    if items.is_empty() {
        return Err(err("texte vide"));
    }
    draw(bytes, page_index, &g, &items)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Watermark {
    pub text: String,
    pub style: TextStyle,
    /// Degrees, counter-clockwise; 45 runs from bottom-left to top-right.
    #[serde(default)]
    pub angle: f32,
    /// 0-based pages; None = every page.
    #[serde(default)]
    pub pages: Option<Vec<u32>>,
}

/// Puts the watermark across the middle of each page.
pub fn add_watermark(bytes: &[u8], w: &Watermark) -> Result<Vec<u8>> {
    let doc = load(bytes)?;
    let mut out = bytes.to_vec();
    for p in targets(&doc, &w.pages) {
        let g = geometry(&doc, p)?;
        let (vw, vh) = g.visual();
        let lines: Vec<&str> = w.text.lines().filter(|l| !l.trim().is_empty()).collect();
        let n = lines.len() as f32;
        let rad = w.angle.to_radians();
        let items = lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                // Lines stacked perpendicular to the writing direction.
                let off = ((n - 1.0) / 2.0 - i as f32) * w.style.size * 1.2;
                Item {
                    text: l.to_string(),
                    style: w.style,
                    u: vw / 2.0 - off * rad.sin(),
                    v: vh / 2.0 + off * rad.cos(),
                    align: Align::Center,
                    angle: w.angle,
                    middle: true,
                }
            })
            .collect::<Vec<_>>();
        out = draw(&out, p, &g, &items)?;
    }
    Ok(out)
}

/// Header and footer texts; each slot may use {page}, {pages}, {date} and {file}.
#[derive(Debug, Clone, Deserialize)]
pub struct HeaderFooter {
    #[serde(default)]
    pub header: [String; 3],
    #[serde(default)]
    pub footer: [String; 3],
    pub style: TextStyle,
    /// Distance from the page edges, in points.
    pub margin: f32,
    /// Number shown on the first changed page.
    #[serde(default = "first")]
    pub start: u32,
    #[serde(default)]
    pub pages: Option<Vec<u32>>,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub file: String,
}

fn first() -> u32 {
    1
}

/// Adds headers, footers and page numbers.
pub fn add_header_footer(bytes: &[u8], hf: &HeaderFooter) -> Result<Vec<u8>> {
    let doc = load(bytes)?;
    let pages = targets(&doc, &hf.pages);
    let total = (hf.start as usize + pages.len()).saturating_sub(1);
    let mut out = bytes.to_vec();
    for (k, &p) in pages.iter().enumerate() {
        let g = geometry(&doc, p)?;
        let (vw, vh) = g.visual();
        let fill = |s: &str| {
            s.replace("{page}", &(hf.start as usize + k).to_string())
                .replace("{pages}", &total.to_string())
                .replace("{date}", &hf.date)
                .replace("{file}", &hf.file)
        };
        let mut items = vec![];
        for (slots, v) in [(&hf.header, vh - hf.margin - hf.style.size * 0.75), (&hf.footer, hf.margin)] {
            for (i, align) in [Align::Left, Align::Center, Align::Right].into_iter().enumerate() {
                let t = fill(&slots[i]);
                if t.trim().is_empty() {
                    continue;
                }
                let u = match align {
                    Align::Left => hf.margin,
                    Align::Center => vw / 2.0,
                    Align::Right => vw - hf.margin,
                };
                items.push(Item { text: t, style: hf.style, u, v, align, angle: 0.0, middle: false });
            }
        }
        out = draw(&out, p, &g, &items)?;
    }
    Ok(out)
}
