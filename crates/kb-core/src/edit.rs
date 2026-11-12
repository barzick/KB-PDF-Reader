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
    /// Distinct styles used in the paragraph (bold, italic, other sizes or colours).
    pub styles: Vec<RunStyle>,
    /// For each char of `text`, its index in `styles`.
    pub char_styles: Vec<u16>,
    /// Lines are stretched to both margins (except the last one).
    pub justified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct RunStyle {
    /// Raw PDF font name.
    pub font: String,
    pub size: f32,
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
    /// For each char of `text`, the index of the fragment it comes from.
    char_frag: Vec<usize>,
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
                    l.char_frag.push(l.frags.len() - 1);
                }
                l.text.push_str(&f.text);
                l.char_frag.extend(std::iter::repeat_n(l.frags.len(), f.text.chars().count()));
                l.right = l.right.max(f.right);
                l.size = l.size.max(f.size);
                l.frags.push(f);
            }
            None => lines.push(Line {
                text: f.text.clone(),
                char_frag: vec![0; f.text.chars().count()],
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
            let same_style = crate::normalize_font_name(&p.font) == crate::normalize_font_name(&font)
                && (p.size - size).abs() < 0.6;
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
            let mut styles: Vec<RunStyle> = vec![];
            let mut style_of = |f: &Fragment| -> u16 {
                let st = RunStyle { font: f.font.clone(), size: (f.size * 10.0).round() / 10.0, color: f.color };
                match styles.iter().position(|x| *x == st) {
                    Some(i) => i as u16,
                    None => {
                        styles.push(st);
                        (styles.len() - 1) as u16
                    }
                }
            };
            let mut text = String::new();
            let mut char_styles: Vec<u16> = vec![];
            for (k, l) in p.lines.iter().enumerate() {
                let chars: Vec<char> = l.text.chars().collect();
                let start = chars.iter().position(|c| !c.is_whitespace()).unwrap_or(chars.len());
                let end = chars.iter().rposition(|c| !c.is_whitespace()).map_or(start, |e| e + 1);
                if start >= end {
                    continue;
                }
                let first_style = style_of(&l.frags[l.char_frag[start]]);
                if k > 0 && !text.is_empty() {
                    let prev = &p.lines[k - 1];
                    let next_word = l.text.split_whitespace().next().unwrap_or("");
                    let needed = (next_word.chars().count() as f32 + 1.0) * 0.5 * p.size;
                    if prev.right + needed < margin - 0.5 * p.size {
                        text.push('\n');
                        char_styles.push(*char_styles.last().unwrap_or(&first_style));
                    } else if !text.ends_with('-') {
                        text.push(' ');
                        char_styles.push(*char_styles.last().unwrap_or(&first_style));
                    }
                }
                for i in start..end {
                    text.push(chars[i]);
                    char_styles.push(style_of(&l.frags[l.char_frag[i]]));
                }
            }
            // Justified: every line but the last reaches the same right edge,
            // and at least one of them would not have ended there naturally.
            let max_right = p.lines.iter().map(|l| l.right).fold(0.0, f32::max);
            let justified = p.lines.len() >= 2
                && p.lines[..p.lines.len() - 1].iter().all(|l| (max_right - l.right).abs() < 0.6)
                && (p.lines.len() > 2 || (max_right - right_margin).abs() < 1.5)
                && !text.contains('\n');
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
                styles,
                char_styles,
                justified,
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

/// Greedy word wrapping with a single style. Explicit '\n' in `text` are kept.
#[cfg(test)]
fn wrap(text: &str, max_width: f32, size: f32, m: &Measure) -> Vec<String> {
    let styles = vec![0u16; text.chars().count()];
    layout(text, &styles, max_width, &[size], &[m], false)
        .into_iter()
        .map(|l| l.pieces.into_iter().map(|p| p.text).collect::<String>())
        .collect()
}

/// Gives each char of `new_text` the style of the matching char in the old
/// text; inserted text takes the style of the text just before it.
fn transfer_styles(old: &str, old_styles: &[u16], new_text: &str) -> Vec<u16> {
    use similar::{capture_diff_slices, Algorithm, DiffOp};
    let a: Vec<char> = old.chars().collect();
    let b: Vec<char> = new_text.chars().collect();
    let style_at = |i: usize| -> u16 { old_styles.get(i).or(old_styles.last()).copied().unwrap_or(0) };
    let mut out = vec![0u16; b.len()];
    for op in capture_diff_slices(Algorithm::Myers, &a, &b) {
        match op {
            DiffOp::Equal { old_index, new_index, len } => {
                for k in 0..len {
                    out[new_index + k] = style_at(old_index + k);
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for k in 0..new_len {
                    out[new_index + k] = style_at(old_index + k.min(old_len.saturating_sub(1)));
                }
            }
            DiffOp::Insert { old_index, new_index, new_len } => {
                let st = if old_index > 0 { style_at(old_index - 1) } else { style_at(0) };
                for k in 0..new_len {
                    out[new_index + k] = st;
                }
            }
            DiffOp::Delete { .. } => {}
        }
    }
    out
}

/// A run of text in one style, at a horizontal offset from the paragraph's left edge.
#[derive(Debug, Clone)]
struct Piece {
    text: String,
    style: u16,
    dx: f32,
}

#[derive(Debug, Clone)]
struct LaidLine {
    pieces: Vec<Piece>,
}

/// Wraps styled text into lines no wider than `max_width`, optionally justified.
fn layout(text: &str, styles: &[u16], max_width: f32, sizes: &[f32], measures: &[&Measure], justify: bool) -> Vec<LaidLine> {
    let chars: Vec<char> = text.chars().collect();
    let size = |s: u16| sizes.get(s as usize).copied().unwrap_or(sizes[0]);
    let measure = |s: u16| measures.get(s as usize).copied().unwrap_or(measures[0]);
    let width = |t: &str, s: u16| measure(s).width(t, size(s));

    // Words as lists of (text, style) segments, plus the style of the space after them.
    struct Word {
        segs: Vec<(String, u16)>,
        width: f32,
        space_style: u16,
    }
    let mut out = vec![];
    let mut i = 0;
    let mut hard_lines: Vec<Vec<Word>> = vec![vec![]];
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            hard_lines.push(vec![]);
            i += 1;
            continue;
        }
        if c == ' ' {
            if let Some(w) = hard_lines.last_mut().unwrap().last_mut() {
                w.space_style = styles[i];
            }
            i += 1;
            continue;
        }
        let mut segs: Vec<(String, u16)> = vec![];
        while i < chars.len() && chars[i] != ' ' && chars[i] != '\n' {
            match segs.last_mut() {
                Some((t, st)) if *st == styles[i] => t.push(chars[i]),
                _ => segs.push((chars[i].to_string(), styles[i])),
            }
            i += 1;
        }
        let w = segs.iter().map(|(t, st)| width(t, *st)).sum();
        hard_lines.last_mut().unwrap().push(Word { segs, width: w, space_style: styles[i.min(styles.len()) - 1] });
    }

    for words in hard_lines {
        // Greedy fill.
        let mut lines: Vec<Vec<&Word>> = vec![vec![]];
        let mut cur = 0.0;
        for w in &words {
            let line = lines.last_mut().unwrap();
            let space = line.last().map(|p: &&Word| width(" ", p.space_style)).unwrap_or(0.0);
            if !line.is_empty() && cur + space + w.width > max_width {
                lines.push(vec![w]);
                cur = w.width;
            } else {
                cur += space + w.width;
                line.push(w);
            }
        }
        let n = lines.len();
        for (li, line) in lines.into_iter().enumerate() {
            let natural: f32 = line.iter().map(|w| w.width).sum::<f32>()
                + line.windows(2).map(|p| width(" ", p[0].space_style)).sum::<f32>();
            let gaps = line.len().saturating_sub(1);
            let extra = if justify && li + 1 < n && gaps > 0 { ((max_width - natural) / gaps as f32).max(0.0) } else { 0.0 };
            let mut pieces: Vec<Piece> = vec![];
            let mut x = 0.0;
            for (wi, w) in line.iter().enumerate() {
                for (t, st) in &w.segs {
                    // Merge with the previous piece when nothing forces a split.
                    match pieces.last_mut() {
                        Some(p) if p.style == *st && extra == 0.0 => p.text.push_str(t),
                        _ => pieces.push(Piece { text: t.clone(), style: *st, dx: x }),
                    }
                    x += width(t, *st);
                }
                if wi + 1 < line.len() {
                    let sp = width(" ", w.space_style);
                    if extra == 0.0 {
                        match pieces.last_mut() {
                            Some(p) if p.style == w.space_style => p.text.push(' '),
                            _ => pieces.push(Piece { text: " ".into(), style: w.space_style, dx: x }),
                        }
                    }
                    x += sp + extra;
                }
            }
            out.push(LaidLine { pieces });
        }
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

fn write_lines(doc: &mut PdfDocument, page_index: PdfPageIndex, block: &TextBlock, lines: &[LaidLine], fonts: &[WriteFont]) -> Result<()> {
    let mut tokens = vec![];
    for f in fonts {
        tokens.push(load_font(doc, f)?);
    }
    let mut page = doc.pages().get(page_index)?;
    page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
    for (n, line) in lines.iter().enumerate() {
        let y = block.y - n as f32 * block.line_height;
        for piece in &line.pieces {
            if piece.text.trim().is_empty() {
                continue;
            }
            let st = &block.styles.get(piece.style as usize).cloned().unwrap_or(RunStyle {
                font: block.font.clone(),
                size: block.size,
                color: block.color,
            });
            let token = tokens.get(piece.style as usize).copied().unwrap_or(tokens[0]);
            let mut obj = page.objects_mut().create_text_object(
                PdfPoints::new(block.x + piece.dx),
                PdfPoints::new(y),
                &piece.text,
                token,
                PdfPoints::new(st.size),
            )?;
            obj.set_fill_color(PdfColor::new(st.color[0], st.color[1], st.color[2], st.color[3]))?;
        }
    }
    page.regenerate_content()?;
    Ok(())
}

/// Some font files can't be embedded by PDFium (e.g. certain OpenType/CFF or
/// variable fonts): fall back to a standard font rather than failing.
fn usable_font<'a>(pdfium: &Pdfium, font: WriteFont<'a>) -> Result<WriteFont<'a>> {
    Ok(match font {
        WriteFont::File(p) => {
            let mut probe = pdfium.create_new_pdf()?;
            if probe.fonts_mut().load_true_type_from_file(p, true).is_ok() {
                WriteFont::File(p)
            } else {
                let name = p.to_string_lossy().to_lowercase();
                let family = if name.contains("mono") || name.contains("cour") {
                    StandardFamily::Mono
                } else if name.contains("serif") && !name.contains("sans") || name.contains("times") {
                    StandardFamily::Serif
                } else {
                    StandardFamily::Sans
                };
                WriteFont::Standard { bold: name.contains("bold"), italic: name.contains("italic") || name.contains("oblique"), family }
            }
        }
        other => other,
    })
}

/// Replaces a paragraph with new text, wrapped to the paragraph's width.
/// `fonts` gives the font for each entry of `block.styles` (a single font is
/// reused for every style).
pub fn replace_block(
    bytes: &[u8],
    page_index: u32,
    block: &TextBlock,
    new_text: &str,
    fonts: Vec<WriteFont>,
    mode: EditMode,
) -> Result<EditResult> {
    let pdfium = pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    let (page_w, page_h) = (page.width().value, page.height().value);
    let text_count = page.objects().iter().filter(|o| o.as_text_object().is_some()).count();

    // Check the objects still match what the user saw.
    let current = build_blocks(build_lines(fragments(&page)), page_w);
    if !current.iter().any(|b| b.objects == block.objects) {
        return Err(KbError::Pdf("le paragraphe a changé, rechargez la page".into()));
    }
    drop(page);
    drop(doc);

    let mut fonts = fonts.into_iter().map(|f| usable_font(pdfium, f)).collect::<Result<Vec<_>>>()?;
    if fonts.is_empty() {
        fonts.push(WriteFont::Standard { bold: false, italic: false, family: StandardFamily::Sans });
    }
    while fonts.len() < block.styles.len().max(1) {
        let first = match &fonts[0] {
            WriteFont::File(p) => WriteFont::File(p),
            WriteFont::Standard { bold, italic, family } => WriteFont::Standard { bold: *bold, italic: *italic, family: *family },
        };
        fonts.push(first);
    }
    let measures: Vec<Measure> = fonts
        .iter()
        .map(|f| Measure {
            face_data: match f {
                WriteFont::File(p) => std::fs::read(p).ok(),
                WriteFont::Standard { .. } => None,
            },
        })
        .collect();
    let measure_refs: Vec<&Measure> = measures.iter().collect();
    let sizes: Vec<f32> = if block.styles.is_empty() {
        vec![block.size]
    } else {
        block.styles.iter().map(|s| s.size).collect()
    };

    // Wrap within the paragraph width; a single line may grow up to the right margin.
    let original_width = block.right - block.x;
    let max_width = if block.lines > 1 {
        original_width * 1.01
    } else {
        (page_w - block.x - 36.0).max(original_width)
    };
    let new_text = new_text.trim_end();
    let styles = if block.char_styles.len() == block.text.chars().count() {
        transfer_styles(&block.text, &block.char_styles, new_text)
    } else {
        vec![0; new_text.chars().count()]
    };
    let lines = layout(new_text, &styles, max_width, &sizes, &measure_refs, block.justified);
    let extra_lines = lines.len() as i32 - block.lines as i32;

    if mode == EditMode::Exact {
        // New text drawn on an empty page of the same size, then merged in.
        let mut scratch = pdfium.create_new_pdf()?;
        scratch
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(page_w), PdfPoints::new(page_h)))?;
        write_lines(&mut scratch, 0, block, &lines, &fonts)?;
        let overlay = scratch.save_to_bytes()?;
        match crate::surgery::remove_and_overlay(bytes, page_index, &block.ordinals, text_count, Some(&overlay)) {
            Ok(out) => return Ok(EditResult { bytes: out, lossless: true, extra_lines }),
            Err(e) => eprintln!("édition exacte impossible ({e}), réécriture de la page"),
        }
    }

    let out = rewrite_page(bytes, page_index, block, &lines, &fonts)?;
    Ok(EditResult { bytes: out, lossless: false, extra_lines })
}

/// Fallback / "shift below" mode: PDFium rewrites the page.
fn rewrite_page(bytes: &[u8], page_index: u32, block: &TextBlock, lines: &[LaidLine], fonts: &[WriteFont]) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut doc = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    {
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
        page.regenerate_content()?;
    }
    write_lines(&mut doc, page_index as PdfPageIndex, block, lines, fonts)?;
    Ok(doc.save_to_bytes()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_styles_through_edits() {
        // "Hello world" with "world" in style 1.
        let old = "Hello world";
        let st = [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1];
        let new = transfer_styles(old, &st, "Hello big world!");
        assert_eq!(new, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn justifies_all_but_last_line() {
        let m = Measure { face_data: None };
        let text = "aa bb cc dd ee ff";
        let st = vec![0u16; text.chars().count()];
        let lines = layout(text, &st, 45.0, &[10.0], &[&m], true);
        // 5 pt per char: "aa bb cc" = 40 pt, stretched to 45 pt.
        assert_eq!(lines.len(), 2);
        let last_piece = lines[0].pieces.last().unwrap();
        assert!((last_piece.dx + 10.0 - 45.0).abs() < 0.01, "first line reaches the right edge: {:?}", lines[0]);
        // The last line keeps its natural spacing.
        let end: f32 = lines[1].pieces.last().map(|p| p.dx + p.text.chars().count() as f32 * 5.0).unwrap();
        assert!((end - 40.0).abs() < 0.01, "last line not stretched: {:?}", lines[1]);
    }

    #[test]
    fn wraps_words() {
        let m = Measure { face_data: None };
        // 0.5 × 10 = 5 pt per char; 40 pt ≈ 8 chars
        assert_eq!(wrap("un deux trois quatre", 40.0, 10.0, &m), vec!["un deux", "trois", "quatre"]);
        assert_eq!(wrap("a\nb", 100.0, 10.0, &m), vec!["a", "b"]);
    }
}
