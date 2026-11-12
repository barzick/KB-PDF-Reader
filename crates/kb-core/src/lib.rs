//! KB PDF Reader engine: everything the app does to a PDF goes through here.
//!
//! PDFium is bundled next to the app (a single shared library) and loaded once
//! at start-up. Documents are kept as bytes by the caller and opened on demand,
//! which keeps this layer stateless and safe to call from any thread.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::OnceLock;

use pdfium_render::prelude::*;
use serde::Serialize;

mod annot;
mod create;
mod forms;
mod decorate;
mod security;
mod optimize;
pub use optimize::{compress, crop, CompressLevel, CompressResult, Crop};
pub use security::{protect, protection, unlock, ProtectOptions, Protection};
pub use decorate::{add_header_footer, add_text, add_watermark, Align, HeaderFooter, TextStyle, Watermark};
pub use forms::{fill_fields, flatten, list_fields, FieldKind, FieldValue, FormField};
mod std14;
pub use annot::{add_annotation, delete_annotation, list_annotations, set_note_text, AnnotInfo, NewAnnot};
mod edit;
mod fonts;
#[cfg(feature = "ocr")]
mod ocr;
mod surgery;
mod text;
pub use text::{page_text, search, PageText, SearchHit};
pub use create::{blank_document, images_to_pdf, insert_blank_page, PageFormat};
#[cfg(feature = "ocr")]
pub use ocr::{make_searchable, ocr_languages, ocr_page, page_needs_ocr, scanned_pages, OcrPage, OcrSummary, OcrWord};
pub use edit::{replace_block, text_blocks, EditMode, EditResult, RunStyle, StandardFamily, TextBlock, WriteFont};
pub use fonts::{normalize_font_name, weight_from_name, FontResolver, FontSource, FontUsage, Resolution};

#[derive(Debug, thiserror::Error)]
pub enum KbError {
    #[error("moteur PDF non initialisé")]
    NotInitialized,
    #[error("impossible de charger le moteur PDF : {0}")]
    Load(String),
    #[error("PDF illisible ou protégé : {0}")]
    Pdf(String),
    #[error("page {0} inexistante")]
    BadPage(u32),
    #[error("image : {0}")]
    Image(String),
}

impl From<PdfiumError> for KbError {
    fn from(e: PdfiumError) -> Self {
        KbError::Pdf(format!("{e:?}"))
    }
}

pub type Result<T> = std::result::Result<T, KbError>;

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();

/// Loads the PDFium library from `lib_dir` (the folder holding pdfium.dll /
/// libpdfium.dylib / libpdfium.so). Falls back to the system library search path.
pub fn init(lib_dir: Option<&Path>) -> Result<()> {
    // Several threads may start at once (tests, background tasks): bind only once.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if PDFIUM.get().is_some() {
        return Ok(());
    }
    let bindings = match lib_dir {
        Some(dir) => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir))
            .or_else(|_| Pdfium::bind_to_system_library()),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|e| KbError::Load(format!("{e:?}")))?;
    let _ = PDFIUM.set(Pdfium::new(bindings));
    Ok(())
}

fn pdfium() -> Result<&'static Pdfium> {
    PDFIUM.get().ok_or(KbError::NotInitialized)
}

fn open(bytes: &[u8]) -> Result<PdfDocument<'_>> {
    Ok(pdfium()?.load_pdf_from_byte_slice(bytes, None)?)
}

#[derive(Debug, Clone, Serialize)]
pub struct PageInfo {
    /// Size in PDF points (1/72 inch), already accounting for page rotation.
    pub width: f32,
    pub height: f32,
    /// Bottom-left corner of the visible area in page coordinates (usually 0, 0).
    pub x0: f32,
    pub y0: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocInfo {
    pub page_count: u32,
    pub pages: Vec<PageInfo>,
    pub title: Option<String>,
}

pub fn info(bytes: &[u8]) -> Result<DocInfo> {
    let doc = open(bytes)?;
    let pages: Vec<PageInfo> = doc
        .pages()
        .iter()
        .map(|p| {
            let b = p.boundaries();
            let r = b.crop().or_else(|_| b.media()).map(|x| x.bounds).ok();
            PageInfo {
                width: p.width().value,
                height: p.height().value,
                x0: r.map(|r| r.left().value).unwrap_or(0.0),
                y0: r.map(|r| r.bottom().value).unwrap_or(0.0),
            }
        })
        .collect();
    let title = doc
        .metadata()
        .get(PdfDocumentMetadataTagType::Title)
        .map(|t| t.value().to_string())
        .filter(|t| !t.trim().is_empty());
    Ok(DocInfo { page_count: pages.len() as u32, pages, title })
}

/// Renders one page (0-based) to PNG at the given pixel width.
pub fn render_png(bytes: &[u8], page_index: u32, target_width: u32) -> Result<Vec<u8>> {
    let doc = open(bytes)?;
    let pages = doc.pages();
    if page_index >= pages.len() as u32 {
        return Err(KbError::BadPage(page_index + 1));
    }
    let page = pages.get(page_index as PdfPageIndex)?;
    let cfg = PdfRenderConfig::new()
        .set_target_width(target_width.clamp(16, 8000) as Pixels)
        .render_form_data(true)
        .render_annotations(true);
    let img = page.render_with_config(&cfg)?.as_image()?;
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| KbError::Image(e.to_string()))?;
    Ok(out)
}

/// Concatenates whole documents in order into a new PDF.
pub fn merge(sources: &[&[u8]]) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut out = pdfium.create_new_pdf()?;
    for src in sources {
        let doc = pdfium.load_pdf_from_byte_slice(src, None)?;
        out.pages_mut().append(&doc)?;
    }
    Ok(out.save_to_bytes()?)
}

/// One page of an assembled document: page `page` (0-based) taken from source `source`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct PageRef {
    pub source: usize,
    pub page: u32,
    /// Extra clockwise rotation in degrees (0, 90, 180, 270).
    #[serde(default)]
    pub rotate: u16,
}

/// Builds a new PDF from an arbitrary list of pages taken from several
/// documents. Covers reorder, delete, extract, split and rotate in one call.
pub fn assemble(sources: &[&[u8]], pages: &[PageRef]) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let docs = sources
        .iter()
        .map(|b| pdfium.load_pdf_from_byte_slice(b, None))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = pdfium.create_new_pdf()?;
    for (i, r) in pages.iter().enumerate() {
        let src = docs.get(r.source).ok_or(KbError::BadPage(r.page + 1))?;
        if r.page >= src.pages().len() as u32 {
            return Err(KbError::BadPage(r.page + 1));
        }
        out.pages_mut().copy_page_from_document(src, r.page as PdfPageIndex, i as PdfPageIndex)?;
        if r.rotate % 360 != 0 {
            let mut page = out.pages().get(i as PdfPageIndex)?;
            let current = rotation_degrees(page.rotation()?);
            page.set_rotation(degrees_to_rotation(current + r.rotate));
        }
    }
    Ok(out.save_to_bytes()?)
}

fn rotation_degrees(r: PdfPageRenderRotation) -> u16 {
    match r {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 90,
        PdfPageRenderRotation::Degrees180 => 180,
        PdfPageRenderRotation::Degrees270 => 270,
    }
}

fn degrees_to_rotation(d: u16) -> PdfPageRenderRotation {
    match (d / 90) % 4 {
        1 => PdfPageRenderRotation::Degrees90,
        2 => PdfPageRenderRotation::Degrees180,
        3 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    }
}

/// Lists every font used by text on the pages, with whether it is embedded.
/// This feeds the font resolver (embedded → system → download → equivalent).
pub fn fonts(bytes: &[u8]) -> Result<Vec<FontUsage>> {
    let doc = open(bytes)?;
    let mut map: BTreeMap<String, FontUsage> = BTreeMap::new();
    for (pi, page) in doc.pages().iter().enumerate() {
        for obj in page.objects().iter() {
            let Some(text) = obj.as_text_object() else { continue };
            let font = text.font();
            let raw = font.name();
            let entry = map.entry(raw.clone()).or_insert_with(|| FontUsage {
                family: normalize_font_name(&raw),
                raw_name: raw.clone(),
                embedded: font.is_embedded().unwrap_or(false),
                bold: font.is_bold_reenforced() || weight_from_name(&raw) >= 600,
                weight: weight_from_name(&raw).max(if font.is_bold_reenforced() { 700 } else { 0 }),
                italic: font.is_italic(),
                pages: Vec::new(),
            });
            let p = pi as u32 + 1;
            if entry.pages.last() != Some(&p) {
                entry.pages.push(p);
            }
        }
    }
    Ok(map.into_values().collect())
}

/// Builds a simple PDF with one line of text per page. Used by tests and as
/// the "new blank document" starting point.
pub fn new_text_document(lines: &[&str]) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut doc = pdfium.create_new_pdf()?;
    let font = doc.fonts_mut().helvetica();
    for line in lines {
        let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4())?;
        page.objects_mut().create_text_object(
            PdfPoints::new(72.0),
            PdfPoints::new(760.0),
            line,
            font,
            PdfPoints::new(24.0),
        )?;
    }
    Ok(doc.save_to_bytes()?)
}
