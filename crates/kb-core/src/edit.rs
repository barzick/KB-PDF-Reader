//! Editing existing text.
//!
//! A PDF stores text as positioned fragments (often one per line, sometimes
//! one per word). We rebuild lines and paragraphs from their positions so the
//! user can edit a whole paragraph, then write it back with automatic line
//! wrapping in the paragraph's font.

use std::path::Path;

use pdfium_render::prelude::*;
use serde::Serialize;

use crate::{open, pdfium, KbError, Result};

#[derive(Debug, Clone)]
struct Fragment {
    index: usize,
    /// Position among the page's text objects only.
    ordinal: usize,
    text: String,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Baseline origin of the text object.
    x: f32,
    y: f32,
    size: f32,
    font: String,
    color: [u8; 4],
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct TextBlock {
    /// Indexes of the page objects that make up this paragraph.
    pub objects: Vec<usize>,
    /// Same objects, counted among text objects only.
    pub ordinals: Vec<usize>,
    /// Paragraph text; line breaks of the original layout are replaced by spaces.
    pub text: String,
    pub lines: usize,
    /// Bounding box in PDF points, origin at the bottom-left of the page.
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
    /// Baseline of the first line.
    pub x: f32,
    pub y: f32,
    pub size: f32,
    /// Distance between baselines (or 1.2 × size for a single line).
    pub line_height: f32,
    /// Dominant font (raw PDF name).
    pub font: String,
    pub color: [u8; 4],
}

fn fragments(page: &PdfPage) -> Vec<Fragment> {
    let mut out = vec![];
    let mut ordinal = 0;
    for (index, obj) in page.objects().iter().enumerate() {
        let Some(t) = obj.as_text_object() else { continue };
        ordinal += 1;
        let ordinal = ordinal - 1;
        let text = t.text();
        if text.trim().is_empty() {
            continue;
        }
        let Ok(b) = obj.bounds() else { continue };
        let (x, y) = t
            .matrix()
            .map(|m| (m.e(), m.f()))
            .unwrap_or((b.left().value, b.bottom().value));
        let c = obj.fill_color().unwrap_or(PdfColor::new(0, 0, 0, 255));
        out.push(Fragment {
            index,
            ordinal,
            text,
            left: b.left().value,
            right: b.right().value,
            top: b.top().value,
            bottom: b.bottom().value,
            x,
            y,
            size: t.scaled_font_size().value.max(1.0),
            font: t.font().name(),
            color: [c.red(), c.green(), c.blue(), c.alpha()],
        });
    }
    out
}

#[derive(Debug, Clone)]
struct Line {
    frags: Vec<Fragment>,
    text: String,
    left: f32,
    right: f32,
    y: f32,
    size: f32,
}

fn dominant<'a>(frags: impl Iterator<Item = &'a Fragment>) -> (String, f32, [u8; 4]) {
    let mut best: Vec<(String, f32, [u8; 4], usize)> = vec![];
    for f in frags {
        let n = f.text.chars().count();
        match best.iter_mut().find(|b| b.0 == f.font && (b.1 - f.size).abs() < 0.5) {
            Some(b) => b.3 += n,
            None => best.push((f.font.clone(), f.size, f.color, n)),
        }
    }
    best.sort_by(|a, b| b.3.cmp(&a.3));
    best.into_iter().next().map(|b| (b.0, b.1, b.2)).unwrap_or_default()
}

fn build_lines(mut frags: Vec<Fragment>) -> Vec<Line> {
    // Top to bottom, then left to right.
    frags.sort_by(|a, b| b.y.total_cmp(&a.y).then(a.x.total_cmp(&b.x)));
    let mut lines: Vec<Line> = vec![];
    for f in frags {
        let joined = lines.iter_mut().rev().take(4).find(|l| {
            (l.y - f.y).abs() < 0.3 * l.size.max(f.size)
                && f.left >= l.left - 1.0
                && f.left - l.right < 1.2 * l.size.max(f.size)
        });
        match joined {
            Some(l) => {
                let gap = f.left - l.right;
                let needs_space = gap > 0.15 * f.size && !l.text.ends_with(' ') && !f.text.starts_with(' ');
                if needs_space {
                    l.text.push(' ');
                }
                l.text.push_str(&f.text);
                l.right = l.right.max(f.right);
                l.size = l.size.max(f.size);
                l.frags.push(f);
            }
            None => lines.push(Line {
                text: f.text.clone(),
                left: f.left,
                right: f.right,
                y: f.y,
                size: f.size,
                frags: vec![f],
            }),
        }
    }
    lines.sort_by(|a, b| b.y.total_cmp(&a.y).then(a.left.total_cmp(&b.left)));
    lines
}

fn build_blocks(lines: Vec<Line>, page_width: f32) -> Vec<TextBlock> {
    // Right margin assumed symmetric to the page's left margin.
    let right_margin = page_width - lines.iter().map(|l| l.left).fold(page_width / 2.0, f32::min);
    struct Para {
        lines: Vec<Line>,
        font: String,
        size: f32,
        spacing: Option<f32>,
    }
    let mut paras: Vec<Para> = vec![];
    for line in lines {
        let (font, size, _) = dominant(line.frags.iter());
        let target = paras.iter_mut().rev().take(6).find(|p| {
            let last = p.lines.last().unwrap();
            let gap = last.y - line.y;
            let same_style = p.font == font && (p.size - size).abs() < 0.6;
            let aligned = (last.left - line.left).abs() < 1.5 * size;
            let close = gap > 0.5 * size && gap < 1.75 * size;
            let regular = p.spacing.is_none_or(|s| (gap - s).abs() < 0.25 * s);
            same_style && aligned && close && regular
        });
        match target {
            Some(p) => {
                let gap = p.lines.last().unwrap().y - line.y;
                p.spacing.get_or_insert(gap);
                p.lines.push(line);
            }
            None => paras.push(Para { lines: vec![line], font, size, spacing: None }),
        }
    }

    paras
        .into_iter()
        .map(|p| {
            let all = || p.lines.iter().flat_map(|l| l.frags.iter());
            let (_, _, color) = dominant(all());
            // A line ending well before the right margin, when the next word would
            // have fitted, was a deliberate line break (addresses, lists…).
            let margin = right_margin.max(p.lines.iter().map(|l| l.right).fold(0.0, f32::max));
            let mut text = String::new();
            for (k, l) in p.lines.iter().enumerate() {
                if k > 0 {
                    let prev = &p.lines[k - 1];
                    let next_word = l.text.split_whitespace().next().unwrap_or("");
                    let needed = (next_word.chars().count() as f32 + 1.0) * 0.5 * p.size;
                    if prev.right + needed < margin - 0.5 * p.size {
                        text.push('\n');
                    } else if !text.ends_with('-') {
                        text.push(' ');
                    }
                }
                text.push_str(l.text.trim());
            }
            let first = &p.lines[0];
            let mut objects: Vec<usize> = all().map(|f| f.index).collect();
            objects.sort_unstable();
            let mut ordinals: Vec<usize> = all().map(|f| f.ordinal).collect();
            ordinals.sort_unstable();
            TextBlock {
                objects,
                ordinals,
                text,
                lines: p.lines.len(),
                left: all().map(|f| f.left).fold(f32::MAX, f32::min),
                bottom: all().map(|f| f.bottom).fold(f32::MAX, f32::min),
                right: all().map(|f| f.right).fold(f32::MIN, f32::max),
                top: all().map(|f| f.top).fold(f32::MIN, f32::max),
                x: first.frags.iter().map(|f| f.x).fold(f32::MAX, f32::min),
                y: first.y,
                size: p.size,
                line_height: p.spacing.unwrap_or(p.size * 1.2),
                font: p.font,
                color,
            }
        })
        .collect()
}

/// Paragraphs of a page (0-based), top to bottom.
pub fn text_blocks(bytes: &[u8], page_index: u32) -> Result<Vec<TextBlock>> {
    let doc = open(bytes)?;
    let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    Ok(build_blocks(build_lines(fragments(&page)), page.width().value))
}

/// Font used to write the new text.
pub enum WriteFont<'a> {
    /// A complete TrueType/OpenType file (found by the font resolver).
    File(&'a Path),
    /// One of the 14 fonts every PDF reader has, when nothing better exists.
    Standard { bold: bool, italic: bool, family: StandardFamily },
}

#[derive(Clone, Copy)]
pub enum StandardFamily {
    Sans,
    Serif,
    Mono,
}

/// Measures text width with the real glyph widths of the font file.
struct Measure {
    face_data: Option<Vec<u8>>,
}

impl Measure {
    fn width(&self, text: &str, size: f32) -> f32 {
        if let Some(data) = &self.face_data {
            if let Ok(face) = ttf_parser::Face::parse(data, 0) {
                let upem = face.units_per_em() as f32;
                let units: f32 = text
                    .chars()
                    .map(|c| {
                        face.glyph_index(c)
                            .and_then(|g| face.glyph_hor_advance(g))
                            .unwrap_or((upem * 0.5) as u16) as f32
                    })
                    .sum();
                return units / upem * size;
            }
        }
        // Average width for standard fonts.
        text.chars().count() as f32 * size * 0.5
    }
}

/// Greedy word wrapping. Explicit '\n' in `text` are kept.
fn wrap(text: &str, max_width: f32, size: f32, m: &Measure) -> Vec<String> {
    let mut out = vec![];
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ').filter(|w| !w.is_empty()) {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && m.width(&candidate, size) > max_width {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EditMode {
    /// Everything else on the page stays exactly as it was. If the new text is
    /// longer, it may run over what is below (like Acrobat).
    #[default]
    Exact,
    /// Moves everything below the paragraph up or down to make room.
    /// The page is rewritten by PDFium, which may slightly change letter spacing elsewhere.
    ShiftBelow,
}

#[derive(Debug, Clone, Serialize)]
pub struct EditResult {
    #[serde(skip)]
    pub bytes: Vec<u8>,
    /// True when the rest of the page was kept exactly as it was.
    pub lossless: bool,
    /// New line count minus original line count.
    pub extra_lines: i32,
}

fn load_font(doc: &mut PdfDocument, font: &WriteFont) -> Result<PdfFontToken> {
    Ok(match font {
        WriteFont::File(p) => doc.fonts_mut().load_true_type_from_file(p, true)?,
        WriteFont::Standard { bold, italic, family } => {
            let f = doc.fonts_mut();
            match (family, bold, italic) {
                (StandardFamily::Sans, false, false) => f.helvetica(),
                (StandardFamily::Sans, true, false) => f.helvetica_bold(),
                (StandardFamily::Sans, false, true) => f.helvetica_oblique(),
                (StandardFamily::Sans, true, true) => f.helvetica_bold_oblique(),
                (StandardFamily::Serif, false, false) => f.times_roman(),
                (StandardFamily::Serif, true, false) => f.times_bold(),
                (StandardFamily::Serif, false, true) => f.times_italic(),
                (StandardFamily::Serif, true, true) => f.times_bold_italic(),
                (StandardFamily::Mono, false, false) => f.courier(),
                (StandardFamily::Mono, true, false) => f.courier_bold(),
                (StandardFamily::Mono, false, true) => f.courier_oblique(),
                (StandardFamily::Mono, true, true) => f.courier_bold_oblique(),
            }
        }
    })
}

fn write_lines(page: &mut PdfPage, block: &TextBlock, lines: &[String], token: PdfFontToken) -> Result<()> {
    let color = PdfColor::new(block.color[0], block.color[1], block.color[2], block.color[3]);
    for (n, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let y = block.y - n as f32 * block.line_height;
        let mut obj = page.objects_mut().create_text_object(
            PdfPoints::new(block.x),
            PdfPoints::new(y),
            line,
            token,
            PdfPoints::new(block.size),
        )?;
        obj.set_fill_color(color)?;
    }
    Ok(())
}

/// Replaces a paragraph with new text, wrapped to the paragraph's width.
pub fn replace_block(
    bytes: &[u8],
    page_index: u32,
    block: &TextBlock,
    new_text: &str,
    font: WriteFont,
    mode: EditMode,
) -> Result<EditResult> {
    let pdfium = pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    let (page_w, page_h) = (page.width().value, page.height().value);
    let text_count = page.objects().iter().filter(|o| o.as_text_object().is_some()).count();

    // Check the objects still match what the user saw.
    let current = build_blocks(build_lines(fragments(&page)), page.width().value);
    if !current.iter().any(|b| b.objects == block.objects) {
        return Err(KbError::Pdf("le paragraphe a changé, rechargez la page".into()));
    }
    drop(page);
    drop(doc);

    let measure = Measure {
        face_data: match &font {
            WriteFont::File(p) => std::fs::read(p).ok(),
            WriteFont::Standard { .. } => None,
        },
    };
    // Wrap within the paragraph width; a single line may grow up to the right margin.
    let original_width = block.right - block.x;
    let max_width = if block.lines > 1 {
        original_width * 1.01
    } else {
        (page_w - block.x - 36.0).max(original_width)
    };
    let lines = wrap(new_text.trim_end(), max_width, block.size, &measure);
    let extra_lines = lines.len() as i32 - block.lines as i32;

    if mode == EditMode::Exact {
        // New text drawn on an empty page of the same size, then merged in.
        let mut scratch = pdfium.create_new_pdf()?;
        let token = load_font(&mut scratch, &font)?;
        {
            let mut p = scratch
                .pages_mut()
                .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(page_w), PdfPoints::new(page_h)))?;
            write_lines(&mut p, block, &lines, token)?;
        }
        let overlay = scratch.save_to_bytes()?;
        match crate::surgery::remove_and_overlay(bytes, page_index, &block.ordinals, text_count, Some(&overlay)) {
            Ok(out) => return Ok(EditResult { bytes: out, lossless: true, extra_lines }),
            Err(e) => eprintln!("édition exacte impossible ({e}), réécriture de la page"),
        }
    }

    let out = rewrite_page(bytes, page_index, block, &lines, &font)?;
    Ok(EditResult { bytes: out, lossless: false, extra_lines })
}

/// Fallback / "shift below" mode: PDFium rewrites the page.
fn rewrite_page(bytes: &[u8], page_index: u32, block: &TextBlock, lines: &[String], font: &WriteFont) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut doc = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    let token = load_font(&mut doc, font)?;
    let mut page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);

    // Move everything below the paragraph. Every other object also gets an
    // explicit (possibly zero) move: PDFium only rewrites the text position
    // correctly for objects it considers modified.
    let delta = (lines.len() as f32 - block.lines as f32) * block.line_height;
    let limit = block.bottom + 0.5;
    let count = page.objects().len();
    for i in 0..count {
        if block.objects.contains(&(i as usize)) {
            continue;
        }
        let mut o = page.objects().get(i)?;
        let is_below = o.bounds().map(|b| b.top().value <= limit).unwrap_or(false);
        let dy = if is_below { -delta } else { 0.0 };
        o.translate(PdfPoints::ZERO, PdfPoints::new(dy))?;
    }
    for &i in block.objects.iter().rev() {
        page.objects_mut().remove_object_at_index(i as PdfPageObjectIndex)?;
    }
    write_lines(&mut page, block, lines, token)?;
    page.regenerate_content()?;
    drop(page);
    Ok(doc.save_to_bytes()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_words() {
        let m = Measure { face_data: None };
        // 0.5 × 10 = 5 pt per char; 40 pt ≈ 8 chars
        assert_eq!(wrap("un deux trois quatre", 40.0, 10.0, &m), vec!["un deux", "trois", "quatre"]);
        assert_eq!(wrap("a\nb", 100.0, 10.0, &m), vec!["a", "b"]);
    }
}
