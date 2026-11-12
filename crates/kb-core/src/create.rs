//! Creating new PDFs: from images (photos, scans) or blank pages.

use std::io::Cursor;

use pdfium_render::prelude::*;
use serde::Deserialize;

use crate::{pdfium, KbError, Result};

/// Page format for new pages.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PageFormat {
    A4,
    A3,
    A5,
    Letter,
    Legal,
    /// Page the size of the image (at its resolution, 150 dpi when unknown).
    Fit,
}

fn format_size(f: PageFormat) -> (f32, f32) {
    match f {
        PageFormat::A4 | PageFormat::Fit => (595.28, 841.89),
        PageFormat::A3 => (841.89, 1190.55),
        PageFormat::A5 => (419.53, 595.28),
        PageFormat::Letter => (612.0, 792.0),
        PageFormat::Legal => (612.0, 1008.0),
    }
}

fn is_jpeg(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// One page per image. With a paper format the page follows the image's
/// orientation and the image is fitted inside `margin` points.
pub fn images_to_pdf(images: &[&[u8]], format: PageFormat, margin: f32) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut doc = pdfium.create_new_pdf()?;
    for (n, bytes) in images.iter().enumerate() {
        let img = image::load_from_memory(bytes).map_err(|e| KbError::Image(format!("image {} : {e}", n + 1)))?;
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let (pw, ph) = if format == PageFormat::Fit {
            (iw * 72.0 / 150.0 + 2.0 * margin, ih * 72.0 / 150.0 + 2.0 * margin)
        } else {
            let (a, b) = format_size(format);
            if iw > ih { (b, a) } else { (a, b) }
        };
        let scale = ((pw - 2.0 * margin) / iw).min((ph - 2.0 * margin) / ih);
        let (w, h) = (iw * scale, ih * scale);
        let mut page = doc
            .pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(pw), PdfPoints::new(ph)))?;
        // JPEG photos/scans are embedded as they are (no quality loss, small file).
        let mut obj = if is_jpeg(bytes) {
            let mut o = PdfPageImageObject::new_from_jpeg_reader(&doc, Cursor::new(bytes.to_vec()))?;
            o.scale(w, h)?;
            o
        } else {
            let mut o = PdfPageImageObject::new(&doc, &img)?;
            o.scale(w, h)?;
            o
        };
        obj.translate(PdfPoints::new((pw - w) / 2.0), PdfPoints::new((ph - h) / 2.0))?;
        page.objects_mut().add_image_object(obj)?;
    }
    Ok(doc.save_to_bytes()?)
}

/// A new document made of `count` blank pages.
pub fn blank_document(count: u32, format: PageFormat, landscape: bool) -> Result<Vec<u8>> {
    let pdfium = pdfium()?;
    let mut doc = pdfium.create_new_pdf()?;
    let (a, b) = format_size(format);
    let (w, h) = if landscape { (b, a) } else { (a, b) };
    for _ in 0..count.max(1) {
        doc.pages_mut()
            .create_page_at_end(PdfPagePaperSize::Custom(PdfPoints::new(w), PdfPoints::new(h)))?;
    }
    Ok(doc.save_to_bytes()?)
}
