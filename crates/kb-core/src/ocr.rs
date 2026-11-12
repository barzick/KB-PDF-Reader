//! OCR of scanned pages with Tesseract (compiled into the app).
//!
//! The page is rendered at 300 dpi, Tesseract finds the words and their boxes,
//! and an invisible text layer is added on top of the scan (like Acrobat's
//! "searchable image"): the page looks the same but its text can be searched,
//! selected, copied, and edited with [`crate::replace_block`].

use std::collections::BTreeMap;
use std::path::Path;

use lopdf::content::{Content, Operation};
use lopdf::{Document, Object};
use pdfium_render::prelude::*;
use serde::Serialize;
use tesseract_rs::{TessPageSegMode, TesseractAPI};

use crate::{pdfium, KbError, Result};

/// Resolution used to read the page.
const DPI: f32 = 300.0;
/// Longest side of the rendered image, to keep memory bounded on huge pages.
const MAX_SIDE: f32 = 7000.0;

/// One recognised word, in PDF points (origin bottom-left).
#[derive(Debug, Clone, Serialize)]
pub struct OcrWord {
    pub text: String,
    pub left: f32,
    pub right: f32,
    /// Baseline of the word's line.
    pub baseline: f32,
    /// Font size that makes the text cover the line's height.
    pub size: f32,
    /// Tesseract confidence, 0–100.
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrPage {
    pub page: u32,
    pub words: Vec<OcrWord>,
    /// Mean word confidence, 0–100 (0 when nothing was found).
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrSummary {
    /// Pages that received a text layer (1-based).
    pub pages: Vec<u32>,
    /// Pages skipped because they already contain text (1-based).
    pub skipped: Vec<u32>,
    pub words: usize,
    pub confidence: f32,
}

/// Languages available in a tessdata folder (e.g. ["deu", "eng", "fra"]).
pub fn ocr_languages(tessdata: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(tessdata)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".traineddata").map(str::to_string))
        .filter(|l| l != "osd")
        .collect();
    out.sort();
    out
}

/// True when the page shows content but has no (visible or invisible) text:
/// typically a scanned sheet.
pub fn page_needs_ocr(bytes: &[u8], page_index: u32) -> Result<bool> {
    let doc = crate::open(bytes)?;
    let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    let mut has_image = false;
    for o in page.objects().iter() {
        match o.object_type() {
            PdfPageObjectType::Text => {
                if o.as_text_object().is_some_and(|t| !t.text().trim().is_empty()) {
                    return Ok(false);
                }
            }
            PdfPageObjectType::Image => has_image = true,
            _ => {}
        }
    }
    Ok(has_image)
}

/// The pages of a document that look scanned (0-based).
pub fn scanned_pages(bytes: &[u8]) -> Result<Vec<u32>> {
    let count = crate::open(bytes)?.pages().len() as u32;
    Ok((0..count).filter(|&p| page_needs_ocr(bytes, p).unwrap_or(false)).collect())
}

struct Engine(TesseractAPI);

impl Engine {
    fn new(tessdata: &Path, langs: &str) -> Result<Self> {
        let api = TesseractAPI::new();
        api.init(tessdata, langs)
            .map_err(|e| KbError::Pdf(format!("OCR : langue « {langs} » indisponible ({e})")))?;
        api.set_page_seg_mode(TessPageSegMode::PSM_AUTO_OSD)
            .or_else(|_| api.set_page_seg_mode(TessPageSegMode::PSM_AUTO))
            .map_err(|e| KbError::Pdf(format!("OCR : {e}")))?;
        // Keep spaces between words as recognised (better for tables and forms).
        let _ = api.set_variable("preserve_interword_spaces", "1");
        Ok(Engine(api))
    }

    fn read(&self, bytes: &[u8], page_index: u32) -> Result<OcrPage> {
        let doc = crate::open(bytes)?;
        let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
        let (w_pt, h_pt) = (page.width().value, page.height().value);
        let scale = (DPI / 72.0).min(MAX_SIDE / w_pt.max(h_pt));
        let cfg = PdfRenderConfig::new()
            .set_target_width(((w_pt * scale).round() as Pixels).max(16))
            .render_form_data(true)
            .render_annotations(false);
        let img = page.render_with_config(&cfg)?.as_image()?.to_luma8();
        let (w, h) = img.dimensions();
        // Actual pixels per point (the target width is rounded).
        let k = w as f32 / w_pt;

        let api = &self.0;
        api.set_image(img.as_raw(), w as i32, h as i32, 1, w as i32)
            .map_err(|e| KbError::Pdf(format!("OCR : {e}")))?;
        let _ = api.set_source_resolution((72.0 * k).round() as i32);
        let tsv = api.get_tsv_text(0).map_err(|e| KbError::Pdf(format!("OCR : {e}")))?;
        let _ = api.clear();

        let words = words_from_tsv(&tsv, k, h_pt);
        let confidence = if words.is_empty() {
            0.0
        } else {
            words.iter().map(|w| w.confidence).sum::<f32>() / words.len() as f32
        };
        Ok(OcrPage { page: page_index + 1, words, confidence })
    }
}

/// Reads the text of one page (without changing the document).
pub fn ocr_page(bytes: &[u8], page_index: u32, tessdata: &Path, langs: &str) -> Result<OcrPage> {
    Engine::new(tessdata, langs)?.read(bytes, page_index)
}

/// Adds an invisible, searchable and editable text layer to the given pages
/// (all scanned-looking pages when `pages` is `None`). Pages that already
/// contain text are left untouched. `progress(done, total)` is called after
/// each page.
pub fn make_searchable(
    bytes: &[u8],
    pages: Option<&[u32]>,
    tessdata: &Path,
    langs: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<(Vec<u8>, OcrSummary)> {
    let count = crate::open(bytes)?.pages().len() as u32;
    let wanted: Vec<u32> = match pages {
        Some(p) => p.iter().copied().filter(|&p| p < count).collect(),
        None => (0..count).collect(),
    };
    let engine = Engine::new(tessdata, langs)?;
    let mut out = bytes.to_vec();
    let mut summary = OcrSummary { pages: vec![], skipped: vec![], words: 0, confidence: 0.0 };
    let mut conf_sum = 0.0;
    for (n, &p) in wanted.iter().enumerate() {
        if !page_needs_ocr(&out, p)? {
            summary.skipped.push(p + 1);
            progress(n + 1, wanted.len());
            continue;
        }
        let read = engine.read(&out, p)?;
        if !read.words.is_empty() {
            out = add_text_layer(&out, p, &read.words)?;
            summary.pages.push(p + 1);
            summary.words += read.words.len();
            conf_sum += read.confidence * read.words.len() as f32;
        }
        progress(n + 1, wanted.len());
    }
    if summary.words > 0 {
        summary.confidence = conf_sum / summary.words as f32;
    }
    Ok((out, summary))
}

/// Parses Tesseract's TSV output into words with PDF coordinates.
/// `k` = pixels per point, `page_h` = page height in points.
fn words_from_tsv(tsv: &str, k: f32, page_h: f32) -> Vec<OcrWord> {
    struct Raw {
        text: String,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        conf: f32,
    }
    // (block, paragraph, line) → line box and its words
    let mut lines: BTreeMap<(u32, u32, u32), ((f32, f32), Vec<Raw>)> = BTreeMap::new();
    for row in tsv.lines() {
        let c: Vec<&str> = row.split('\t').collect();
        if c.len() < 12 || c[0] == "level" {
            continue;
        }
        let n = |i: usize| c[i].trim().parse::<f32>().unwrap_or(0.0);
        let key = (n(2) as u32, n(3) as u32, n(4) as u32);
        match c[0] {
            "4" => {
                lines.entry(key).or_insert_with(|| ((n(7), n(7) + n(9)), vec![])).0 = (n(7), n(7) + n(9));
            }
            "5" => {
                let text = c[11..].join("\t").trim().to_string();
                if text.is_empty() || n(10) < 0.0 {
                    continue;
                }
                let e = lines.entry(key).or_insert_with(|| ((n(7), n(7) + n(9)), vec![]));
                e.1.push(Raw { text, left: n(6), top: n(7), right: n(6) + n(8), bottom: n(7) + n(9), conf: n(10) });
            }
            _ => {}
        }
    }

    // Per line: baseline and size; then one size per paragraph (lines of a
    // paragraph share a size, which keeps them together when editing).
    let mut laid: Vec<((u32, u32), f32, f32, Vec<Raw>)> = vec![];
    for ((b, p, _), ((line_top, line_bottom), words)) in lines {
        if words.is_empty() {
            continue;
        }
        // Baseline: where most words end (words with descenders end lower).
        let tall: Vec<&Raw> = {
            let max_h = words.iter().map(|w| w.bottom - w.top).fold(0.0, f32::max);
            words.iter().filter(|w| w.bottom - w.top >= 0.4 * max_h).collect()
        };
        let mut bottoms: Vec<f32> = tall.iter().map(|w| w.bottom).collect();
        bottoms.sort_by(f32::total_cmp);
        let baseline_px = bottoms[(bottoms.len() - 1) / 2].min(line_bottom);
        // Top of the line's letters, ignoring a few taller ones (accented capitals).
        let mut tops: Vec<f32> = tall.iter().map(|w| w.top).collect();
        tops.sort_by(f32::total_cmp);
        let top_px = tops[tops.len() / 4].max(line_top);
        // Cap/ascender height ≈ 0.72 em for common sans-serif fonts.
        let size = ((baseline_px - top_px) / k / 0.72).clamp(2.0, 200.0);
        laid.push(((b, p), size, page_h - baseline_px / k, words));
    }
    let mut para_size: BTreeMap<(u32, u32), f32> = BTreeMap::new();
    for key in laid.iter().map(|l| l.0).collect::<std::collections::BTreeSet<_>>() {
        let mut sizes: Vec<f32> = laid.iter().filter(|l| l.0 == key).map(|l| l.1).collect();
        sizes.sort_by(f32::total_cmp);
        let median = sizes[sizes.len() / 2];
        para_size.insert(key, median);
    }

    let mut out = vec![];
    for (key, own, baseline, words) in laid {
        let median = para_size[&key];
        // Keep a line's own size only when it clearly differs (e.g. a title).
        let size = if (own - median).abs() <= 0.15 * median { median } else { own };
        for w in words {
            out.push(OcrWord {
                text: w.text,
                left: w.left / k,
                right: w.right / k,
                baseline,
                size,
                confidence: w.conf,
            });
        }
    }
    out
}

/// Draws the words invisibly on top of the page, without touching anything
/// else in it.
fn add_text_layer(bytes: &[u8], page_index: u32, words: &[OcrWord]) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let (w_pt, h_pt) = {
        let doc = crate::open(bytes)?;
        let page = doc.pages().get(page_index as PdfPageIndex)?;
        (page.width().value, page.height().value)
    };
    let mut scratch = pdfium.create_new_pdf()?;
    let font = scratch.fonts_mut().helvetica();
    {
        let mut page = scratch
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(w_pt), PdfPoints::new(h_pt)))?;
        page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        for w in words {
            let mut obj =
                page.objects_mut()
                    .create_text_object(PdfPoints::ZERO, PdfPoints::ZERO, &w.text, font, PdfPoints::new(w.size))?;
            // Stretch the word so that selecting it highlights the scanned word.
            let natural = obj.bounds().map(|b| b.right().value - b.left().value).unwrap_or(0.0);
            let target = w.right - w.left;
            let sx = if natural > 0.1 && target > 0.1 { (target / natural).clamp(0.2, 5.0) } else { 1.0 };
            obj.scale(sx, 1.0)?;
            let left_after = obj.bounds().map(|b| b.left().value).unwrap_or(0.0);
            obj.translate(PdfPoints::new(w.left - left_after), PdfPoints::new(w.baseline))?;
        }
        page.regenerate_content()?;
    }
    let overlay = invisible(&scratch.save_to_bytes()?)?;
    crate::surgery::remove_and_overlay(bytes, page_index, &[], 0, Some(&overlay))
}

/// Sets text rendering mode 3 (invisible) on every text object of a one-page PDF.
fn invisible(pdf: &[u8]) -> Result<Vec<u8>> {
    let err = |e: lopdf::Error| KbError::Pdf(e.to_string());
    let mut doc = Document::load_mem(pdf).map_err(err)?;
    let page_id = *doc.get_pages().get(&1).ok_or(KbError::BadPage(1))?;
    let content = Content::decode(&doc.get_page_content(page_id).map_err(err)?).map_err(err)?;
    let mut ops = Vec::with_capacity(content.operations.len() + 64);
    for mut op in content.operations {
        // PDFium writes its own "0 Tr" inside each text object: override it too.
        if op.operator == "Tr" {
            op.operands = vec![Object::Integer(3)];
        }
        let bt = op.operator == "BT";
        ops.push(op);
        if bt {
            ops.push(Operation::new("Tr", vec![Object::Integer(3)]));
        }
    }
    let data = Content { operations: ops }.encode().map_err(err)?;
    doc.change_page_content(page_id, data).map_err(err)?;
    let mut out = vec![];
    doc.save_to(&mut out).map_err(|e| KbError::Pdf(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_to_points() {
        // 2 px per point, page 100 pt high. Line 1: "Bonjour" + "joyeux" (with descender).
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
4\t1\t1\t1\t1\t0\t20\t40\t200\t40\t-1\t\n\
5\t1\t1\t1\t1\t1\t20\t40\t80\t30\t96\tBonjour\n\
5\t1\t1\t1\t1\t2\t110\t42\t70\t38\t90\tjoyeux\n\
5\t1\t1\t1\t1\t3\t190\t42\t10\t10\t-1\t \n";
        let w = words_from_tsv(tsv, 2.0, 100.0);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].text, "Bonjour");
        assert!((w[0].left - 10.0).abs() < 1e-3 && (w[0].right - 50.0).abs() < 1e-3);
        // Both words share the baseline of the median bottom.
        assert_eq!(w[0].baseline, w[1].baseline);
        assert!(w[0].baseline > 50.0 && w[0].baseline < 66.0, "{}", w[0].baseline);
        assert!(w[0].size > 8.0 && w[0].size < 30.0, "{}", w[0].size);
    }
}
