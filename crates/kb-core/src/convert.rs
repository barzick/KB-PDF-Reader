//! Conversions: PDF to images, plain text, Word (.docx) and Excel (.xlsx).

use std::io::{Cursor, Write};

use image::codecs::jpeg::JpegEncoder;
use image::DynamicImage;
use pdfium_render::prelude::*;
use rust_xlsxwriter::{Format, Workbook};
use serde::Deserialize;

use crate::edit::{page_blocks, TextBlock};
use crate::fonts::{normalize_font_name, weight_from_name};
use crate::{open, KbError, Result};

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

fn get<'a>(doc: &'a PdfDocument, index: u32) -> Result<PdfPage<'a>> {
    doc.pages().get(index as PdfPageIndex).map_err(|_| KbError::BadPage(index + 1))
}

/// Visible area of a page: [left, bottom, right, top] in page coordinates.
fn area(page: &PdfPage) -> [f32; 4] {
    let b = page.boundaries();
    match b.crop().or_else(|_| b.media()) {
        Ok(r) => [r.bounds.left().value, r.bounds.bottom().value, r.bounds.right().value, r.bounds.top().value],
        Err(_) => [0.0, 0.0, page.width().value, page.height().value],
    }
}

// ---------------------------------------------------------------- images

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }
}

fn encode(img: &DynamicImage, kind: ImageKind, quality: u8) -> Result<Vec<u8>> {
    let mut out = vec![];
    match kind {
        ImageKind::Png => img
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| KbError::Image(e.to_string()))?,
        ImageKind::Jpeg => JpegEncoder::new_with_quality(Cursor::new(&mut out), quality)
            .encode_image(&DynamicImage::ImageRgb8(img.to_rgb8()))
            .map_err(|e| KbError::Image(e.to_string()))?,
    }
    Ok(out)
}

/// Renders the given pages (0-based) at `dpi`; `each(n, page, data)` receives
/// every image as soon as it is ready.
pub fn to_images(
    bytes: &[u8],
    pages: &[u32],
    dpi: u32,
    kind: ImageKind,
    mut each: impl FnMut(usize, u32, Vec<u8>) -> Result<()>,
) -> Result<()> {
    let doc = open(bytes)?;
    for (n, &p) in pages.iter().enumerate() {
        let page = get(&doc, p)?;
        let px = (page.width().value / 72.0 * dpi.clamp(36, 1200) as f32).round() as i32;
        let cfg = PdfRenderConfig::new()
            .set_target_width(px.clamp(16, 20_000) as Pixels)
            .set_maximum_height(20_000)
            .render_form_data(true)
            .render_annotations(true);
        let img = page.render_with_config(&cfg)?.as_image()?;
        each(n, p, encode(&img, kind, 90)?)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- text

/// Plain text of the given pages, one blank line between pages.
pub fn to_text(bytes: &[u8], pages: &[u32]) -> Result<String> {
    let doc = open(bytes)?;
    let mut out = String::new();
    for &p in pages {
        let text = crate::text::read(&get(&doc, p)?).text;
        let lines: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
        let page = lines.join("\n");
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(page.trim_matches('\n'));
    }
    out.push('\n');
    Ok(out)
}

// ---------------------------------------------------------------- Word

fn xml(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            // Control characters are not allowed in XML.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' => {}
            c => o.push(c),
        }
    }
    o
}

/// Font name Word knows for the usual PDF base fonts.
fn word_font(raw: &str) -> String {
    let n = normalize_font_name(raw);
    let l = n.to_lowercase();
    if l.starts_with("helvetica") || l.starts_with("arialmt") || l == "arial" {
        "Arial".into()
    } else if l.starts_with("times") {
        "Times New Roman".into()
    } else if l.starts_with("courier") {
        "Courier New".into()
    } else if n.is_empty() {
        "Calibri".into()
    } else {
        n
    }
}

fn run_props(font: &str, size: f32, color: [u8; 4]) -> String {
    let l = font.to_lowercase();
    let mut p = format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>", f = xml(&word_font(font)));
    if weight_from_name(font) >= 600 {
        p.push_str("<w:b/>");
    }
    if l.contains("italic") || l.contains("oblique") {
        p.push_str("<w:i/>");
    }
    if color[..3] != [0, 0, 0] {
        p.push_str(&format!("<w:color w:val=\"{:02X}{:02X}{:02X}\"/>", color[0], color[1], color[2]));
    }
    let half = (size * 2.0).round().clamp(2.0, 3276.0) as u32;
    p.push_str(&format!("<w:sz w:val=\"{half}\"/><w:szCs w:val=\"{half}\"/>"));
    p
}

enum Item {
    Text(TextBlock),
    Image { left: f32, bottom: f32, top: f32, width: f32, height: f32, media: usize },
    /// Rows, and the extent of each column used.
    Table { rows: Vec<Row>, cols: Vec<(f32, f32)> },
}

impl Item {
    fn table_box(rows: &[Row], cols: &[(f32, f32)]) -> [f32; 4] {
        [
            cols.first().map(|c| c.0).unwrap_or(0.0),
            rows.last().map(|r| r.bottom).unwrap_or(0.0),
            cols.last().map(|c| c.1).unwrap_or(0.0),
            rows.first().map(|r| r.top).unwrap_or(0.0),
        ]
    }
}

impl Item {
    fn top(&self) -> f32 {
        match self {
            Item::Text(b) => b.top,
            Item::Image { top, .. } => *top,
            Item::Table { rows, cols } => Item::table_box(rows, cols)[3],
        }
    }
    fn bottom(&self) -> f32 {
        match self {
            Item::Text(b) => b.bottom,
            Item::Image { bottom, .. } => *bottom,
            Item::Table { rows, cols } => Item::table_box(rows, cols)[1],
        }
    }
    fn left(&self) -> f32 {
        match self {
            Item::Text(b) => b.left,
            Item::Image { left, .. } => *left,
            Item::Table { rows, cols } => Item::table_box(rows, cols)[0],
        }
    }
    fn right(&self) -> f32 {
        match self {
            Item::Text(b) => b.right,
            Item::Image { left, width, .. } => left + width,
            Item::Table { rows, cols } => Item::table_box(rows, cols)[2],
        }
    }
}

const TW: f32 = 20.0; // twips per point
const EMU: f32 = 12_700.0; // EMU per point

/// A Word document with the text (fonts, sizes, colours, bold, italic,
/// alignment) and the images of the given pages, one section per page.
/// Text flows: the layout is close to the PDF, not identical.
pub fn to_docx(bytes: &[u8], pages: &[u32], mut progress: impl FnMut(usize, usize)) -> Result<Vec<u8>> {
    let doc = open(bytes)?;
    let mut body = String::new();
    // (file name, data)
    let mut media: Vec<(String, Vec<u8>)> = vec![];
    for (n, &p) in pages.iter().enumerate() {
        let page = get(&doc, p)?;
        let [x0, y0, x1, y1] = area(&page);
        let (pw, ph) = (x1 - x0, y1 - y0);
        let blocks = page_blocks(&page);
        let scanned = blocks.iter().any(|b| b.scanned);
        let mut items: Vec<Item> = vec![];
        for obj in page.objects().iter() {
            let Some(im) = obj.as_image_object() else { continue };
            let Ok(b) = obj.bounds() else { continue };
            let (l, bt, r, t) = (b.left().value, b.bottom().value, b.right().value, b.top().value);
            let (w, h) = (r - l, t - bt);
            if w < 12.0 || h < 12.0 {
                continue;
            }
            // The scan behind recognised text would repeat the text.
            if scanned && w * h > pw * ph * 0.6 {
                continue;
            }
            let Ok(img) = im.get_processed_image(&doc).or_else(|_| im.get_raw_image()) else { continue };
            let photo = !img.color().has_alpha() && img.width() * img.height() > 200_000;
            let (data, ext) = if photo {
                (encode(&img, ImageKind::Jpeg, 88)?, "jpeg")
            } else {
                (encode(&img, ImageKind::Png, 0)?, "png")
            };
            media.push((format!("image{}.{ext}", media.len() + 1), data));
            items.push(Item::Image { left: l, bottom: bt, top: t, width: w, height: h, media: media.len() });
        }
        let tables = tables(grid(cells(&page)));
        let boxes: Vec<[f32; 4]> = tables.iter().map(|(r, c)| Item::table_box(r, c)).collect();
        // Paragraphs inside a table are replaced by the table.
        let inside = |b: &TextBlock| {
            boxes.iter().any(|t| {
                let v = b.top.min(t[3]) - b.bottom.max(t[1]);
                let h = b.right.min(t[2]) - b.left.max(t[0]);
                h > 0.0 && v > 0.5 * (b.top - b.bottom)
            })
        };
        items.extend(blocks.into_iter().filter(|b| !b.text.trim().is_empty() && !inside(b)).map(Item::Text));
        items.extend(tables.into_iter().map(|(rows, cols)| Item::Table { rows, cols }));
        items.sort_by(|a, b| b.top().total_cmp(&a.top()));

        // Margins follow the content, within reason.
        let clamp = |v: f32, max: f32| v.clamp(18.0, max);
        let ml = clamp(items.iter().map(|i| i.left() - x0).fold(f32::MAX, f32::min).min(72.0), pw * 0.3);
        let mr = clamp(items.iter().map(|i| x1 - i.right()).fold(f32::MAX, f32::min).min(72.0), pw * 0.3);
        let mt = clamp(items.iter().map(|i| y1 - i.top()).fold(f32::MAX, f32::min).min(72.0), ph * 0.3);
        let mb = clamp(items.iter().map(|i| i.bottom() - y0).fold(f32::MAX, f32::min).min(54.0), ph * 0.3);
        let text_w = pw - ml - mr;
        let center = x0 + ml + text_w / 2.0;

        let mut prev_bottom: Option<f32> = None;
        for it in &items {
            let before = prev_bottom.map(|pb| (pb - it.top()).clamp(0.0, 72.0)).unwrap_or(0.0);
            prev_bottom = Some(it.bottom());
            match it {
                Item::Text(b) => {
                    let mut ppr = format!("<w:spacing w:before=\"{}\" w:after=\"0\"", (before * TW) as i32);
                    if b.lines > 1 && b.size > 0.0 {
                        let line = (240.0 * b.line_height / (1.17 * b.size)).clamp(200.0, 600.0) as i32;
                        ppr.push_str(&format!(" w:line=\"{line}\" w:lineRule=\"auto\""));
                    }
                    ppr.push_str("/>");
                    let width = b.right - b.left;
                    let mid = (b.left + b.right) / 2.0;
                    if b.justified {
                        ppr.push_str("<w:jc w:val=\"both\"/>");
                    } else if (mid - center).abs() < 6.0 && width < text_w * 0.85 && b.left - x0 > ml + 12.0 {
                        ppr.push_str("<w:jc w:val=\"center\"/>");
                    } else if (x1 - mr - b.right).abs() < 4.0 && b.left > center {
                        ppr.push_str("<w:jc w:val=\"right\"/>");
                    } else {
                        let indent = b.left - x0 - ml;
                        if indent > 2.0 {
                            ppr.push_str(&format!("<w:ind w:left=\"{}\"/>", (indent * TW) as i32));
                        }
                    }
                    body.push_str(&format!("<w:p><w:pPr>{ppr}</w:pPr>"));
                    // Short lines (an address, a list) keep their breaks;
                    // the lines of a running paragraph flow again.
                    let lines: Vec<usize> = b.text.split('\n').map(|l| l.chars().count()).collect();
                    let longest = lines.iter().copied().max().unwrap_or(0) as f32;
                    let keep_breaks = lines.len() > 1
                        && !b.justified
                        && (width < text_w * 0.6 || lines[..lines.len() - 1].iter().any(|&n| (n as f32) < longest * 0.7));
                    // Runs of the same style.
                    let chars: Vec<char> = b.text.chars().collect();
                    let mut start = 0;
                    while start < chars.len() {
                        let st = b.char_styles.get(start).copied().unwrap_or(0);
                        let mut end = start + 1;
                        while end < chars.len() && b.char_styles.get(end).copied().unwrap_or(0) == st {
                            end += 1;
                        }
                        let (font, size, color) = match b.styles.get(st as usize) {
                            Some(s) => (s.font.as_str(), s.size, s.color),
                            None => (b.font.as_str(), b.size, b.color),
                        };
                        let text: String = chars[start..end].iter().collect();
                        let text = if keep_breaks {
                            xml(&text).replace('\n', "</w:t><w:br/><w:t xml:space=\"preserve\">")
                        } else {
                            xml(&text.replace('\n', " "))
                        };
                        body.push_str(&format!(
                            "<w:r><w:rPr>{}</w:rPr><w:t xml:space=\"preserve\">{text}</w:t></w:r>",
                            run_props(font, size, color),
                        ));
                        start = end;
                    }
                    body.push_str("</w:p>");
                }
                Item::Table { rows, cols } => {
                    if before > 4.0 {
                        body.push_str(&format!(
                            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{}\" w:lineRule=\"exact\"/></w:pPr></w:p>",
                            (before * TW) as i32
                        ));
                    }
                    body.push_str(&table_xml(rows, cols, x0 + ml, text_w));
                }
                Item::Image { left, width, height, media: id, .. } => {
                    let k = (text_w / width).min(1.0);
                    let (cx, cy) = ((width * k * EMU) as i64, (height * k * EMU) as i64);
                    let indent = (left - x0 - ml).clamp(0.0, text_w - width * k);
                    let ind = if indent > 2.0 { format!("<w:ind w:left=\"{}\"/>", (indent * TW) as i32) } else { String::new() };
                    body.push_str(&format!(
                        "<w:p><w:pPr><w:spacing w:before=\"{b}\" w:after=\"0\"/>{ind}</w:pPr><w:r><w:drawing>\
<wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Image {id}\"/>\
<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"rIdImg{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>\
</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
                        b = (before * TW) as i32,
                        name = media[id - 1].0,
                    ));
                }
            }
        }
        // Section (one per page): its properties go in the last paragraph,
        // except for the very last section, which closes the body.
        let orient = if pw > ph { " w:orient=\"landscape\"" } else { "" };
        let sect = format!(
            "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>",
            (pw * TW) as i32,
            (ph * TW) as i32,
            (mt * TW) as i32,
            (mr * TW) as i32,
            (mb * TW) as i32,
            (ml * TW) as i32,
        );
        if n + 1 < pages.len() {
            body.push_str(&format!("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/>{sect}</w:pPr></w:p>"));
        } else {
            // The body must end with a paragraph.
            if items.last().is_none_or(|i| matches!(i, Item::Table { .. })) {
                body.push_str("<w:p/>");
            }
            body.push_str(&sect);
        }
        progress(n + 1, pages.len());
    }

    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\"><w:body>{body}</w:body></w:document>"
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdStyles\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
    );
    for (i, (name, _)) in media.iter().enumerate() {
        rels.push_str(&format!(
            "<Relationship Id=\"rIdImg{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{name}\"/>",
            i + 1
        ));
    }
    rels.push_str("</Relationships>");
    let styles = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:docDefaults><w:rPrDefault><w:rPr>\
<w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\" w:cs=\"Arial\"/><w:sz w:val=\"22\"/><w:lang w:val=\"fr-FR\"/></w:rPr></w:rPrDefault>\
<w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style></w:styles>";
    let types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>";
    let root_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let parts: [(&str, &[u8]); 5] = [
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", root_rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
    ];
    for (name, data) in parts {
        zip.start_file(name, deflate).map_err(err)?;
        zip.write_all(data).map_err(err)?;
    }
    for (name, data) in &media {
        zip.start_file(format!("word/media/{name}"), stored).map_err(err)?;
        zip.write_all(data).map_err(err)?;
    }
    Ok(zip.finish().map_err(err)?.into_inner())
}

// ---------------------------------------------------------------- tables

/// A piece of text on one line, separated from its neighbours by a wide gap.
#[derive(Debug, Clone)]
struct Cell {
    text: String,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
    font: String,
    size: f32,
    color: [u8; 4],
}

/// Cuts the page text into cells: a new cell starts at each line break and
/// at each horizontal gap wider than about one character.
fn cells(page: &PdfPage) -> Vec<Cell> {
    let Ok(text) = page.text() else { return vec![] };
    let mut out: Vec<Cell> = vec![];
    let mut cur: Option<Cell> = None;
    let mut space = false;
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else { continue };
        if c == '\r' {
            continue;
        }
        if c == '\n' {
            out.extend(cur.take());
            space = false;
            continue;
        }
        if ch.is_generated().unwrap_or(false) || c.is_whitespace() {
            space = true;
            continue;
        }
        let Ok(r) = ch.loose_bounds() else { continue };
        let b = [r.left().value, r.bottom().value, r.right().value, r.top().value];
        if b[2] <= b[0] {
            continue;
        }
        let h = (b[3] - b[1]).max(1.0);
        let joins = cur.as_ref().is_some_and(|k| {
            let mid = (b[1] + b[3]) / 2.0;
            let gap = b[0] - k.right;
            mid > k.bottom && mid < k.top && gap < h * 0.9 && gap > -h
        });
        if joins {
            let k = cur.as_mut().unwrap();
            if space {
                k.text.push(' ');
            }
            k.text.push(c);
            k.right = k.right.max(b[2]);
            k.bottom = k.bottom.min(b[1]);
            k.top = k.top.max(b[3]);
        } else {
            out.extend(cur.take());
            let col = ch.fill_color().map(|c| [c.red(), c.green(), c.blue(), c.alpha()]).unwrap_or([0, 0, 0, 255]);
            cur = Some(Cell {
                text: c.to_string(),
                left: b[0],
                right: b[2],
                bottom: b[1],
                top: b[3],
                font: ch.font_name(),
                size: ch.scaled_font_size().value,
                color: col,
            });
        }
        space = false;
    }
    out.extend(cur);
    out
}

struct Row {
    top: f32,
    bottom: f32,
    /// (column, cell), left to right, at most one cell per column.
    cells: Vec<(usize, Cell)>,
}

struct Grid {
    rows: Vec<Row>,
    /// Horizontal extent of each column.
    bands: Vec<(f32, f32)>,
}

fn band_of(bands: &[(f32, f32)], c: &Cell) -> usize {
    let mut best = (0usize, 0.0f32);
    for (i, b) in bands.iter().enumerate() {
        let o = c.right.min(b.1) - c.left.max(b.0);
        if o > best.1 {
            best = (i, o);
        }
    }
    if best.1 > 0.0 {
        best.0
    } else {
        bands.iter().rposition(|b| b.0 <= c.left).unwrap_or(0)
    }
}

/// Cells grouped in rows (top to bottom) and columns (shared across the page).
fn grid(mut cells: Vec<Cell>) -> Grid {
    cells.sort_by(|a, b| b.top.total_cmp(&a.top));
    let mut lines: Vec<Vec<Cell>> = vec![];
    for c in cells {
        let mid = (c.bottom + c.top) / 2.0;
        match lines.last_mut() {
            Some(r) if r.iter().any(|k| mid > k.bottom && mid < k.top) => r.push(c),
            _ => lines.push(vec![c]),
        }
    }
    for r in &mut lines {
        r.sort_by(|a, b| a.left.total_cmp(&b.left));
    }
    // Column bands: horizontal extents of the cells of multi-cell rows, merged where they overlap…
    let multi: Vec<&Vec<Cell>> = lines.iter().filter(|r| r.len() > 1).collect();
    let mut spans: Vec<(f32, f32)> = multi.iter().flat_map(|r| r.iter()).map(|c| (c.left, c.right)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut bands: Vec<(f32, f32)> = vec![];
    for (l, r) in spans {
        match bands.last_mut() {
            Some(b) if l <= b.1 => b.1 = b.1.max(r),
            _ => bands.push((l, r)),
        }
    }
    // …and neighbours that never appear in the same row (a left-aligned
    // heading above right-aligned numbers) are one column.
    'merge: loop {
        for i in 0..bands.len().saturating_sub(1) {
            let together = multi.iter().any(|r| {
                let cols: Vec<usize> = r.iter().map(|c| band_of(&bands, c)).collect();
                cols.contains(&i) && cols.contains(&(i + 1))
            });
            if !together {
                bands[i].1 = bands[i].1.max(bands[i + 1].1);
                bands.remove(i + 1);
                continue 'merge;
            }
        }
        break;
    }
    let rows = lines
        .into_iter()
        .map(|r| {
            let single = r.len() == 1;
            let mut row = Row {
                top: r.iter().map(|c| c.top).fold(f32::MIN, f32::max),
                bottom: r.iter().map(|c| c.bottom).fold(f32::MAX, f32::min),
                cells: vec![],
            };
            for c in r {
                // A lone line of text (title, sentence) starts in the first column.
                let col = if bands.is_empty() || single && number(&c.text).is_none() { 0 } else { band_of(&bands, &c) };
                match row.cells.last_mut() {
                    Some((k, prev)) if *k >= col => {
                        prev.text.push(' ');
                        prev.text.push_str(&c.text);
                        prev.right = prev.right.max(c.right);
                    }
                    _ => row.cells.push((col, c)),
                }
            }
            row
        })
        .collect();
    Grid { rows, bands }
}

/// A number written the French, Swiss or English way ("1 250,50", "1'250.50",
/// "1,250.50", "-3,5", "(12)"), with its number of decimals. Codes, dates,
/// phone numbers and numbers with leading zeros stay text.
fn number(s: &str) -> Option<(f64, usize)> {
    let mut t: String = s.trim().to_string();
    for sym in ["CHF", "EUR", "USD", "€", "$", "£", "Fr."] {
        t = t.trim_start_matches(sym).trim_end_matches(sym).trim().to_string();
    }
    let (neg, t) = if t.starts_with('(') && t.ends_with(')') {
        (true, t[1..t.len() - 1].to_string())
    } else if let Some(r) = t.strip_prefix('-').or_else(|| t.strip_prefix('−')) {
        (true, r.trim().to_string())
    } else {
        (false, t)
    };
    if t.is_empty() || t.len() > 24 {
        return None;
    }
    // Decimal separator: the last of '.' and ',' when both appear; otherwise a
    // single one (a single ',' is decimal, as in French; repeated ones group thousands).
    let dots = t.matches('.').count();
    let commas = t.matches(',').count();
    let dec = match (dots, commas) {
        (0, 0) => None,
        (_, 0) if dots == 1 => Some('.'),
        (0, _) if commas == 1 => Some(','),
        (_, 0) | (0, _) => None,
        _ => {
            if t.rfind('.') > t.rfind(',') {
                Some('.')
            } else {
                Some(',')
            }
        }
    };
    let (int, frac) = match dec {
        Some(d) => {
            let i = t.rfind(d).unwrap();
            (&t[..i], &t[i + 1..])
        }
        None => (t.as_str(), ""),
    };
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Thousands groups: 1–3 digits, then groups of exactly 3.
    let groups: Vec<&str> = int.split([' ', '\u{a0}', '\u{202f}', '\'', '’', '.', ',']).collect();
    if groups.iter().any(|g| g.is_empty() || !g.chars().all(|c| c.is_ascii_digit()))
        || groups[0].len() > 3 && groups.len() > 1
        || groups[1..].iter().any(|g| g.len() != 3)
    {
        return None;
    }
    let digits: String = groups.concat();
    if digits.len() > 1 && digits.starts_with('0') || digits.len() > 15 {
        return None;
    }
    let v: f64 = format!("{digits}.{}", if frac.is_empty() { "0" } else { frac }).parse().ok()?;
    Some((if neg { -v } else { v }, frac.len()))
}

/// Runs of at least two rows that each have two or more short cells: tables.
/// Returns their rows and the columns they use.
fn tables(g: Grid) -> Vec<(Vec<Row>, Vec<(f32, f32)>)> {
    let tabular = |r: &Row| r.cells.len() >= 2 && r.cells.iter().all(|(_, c)| c.text.chars().count() <= 40);
    let mut out = vec![];
    let mut run: Vec<Row> = vec![];
    let mut flush = |run: &mut Vec<Row>| {
        if run.len() >= 2 {
            let mut used: Vec<usize> = run.iter().flat_map(|r| r.cells.iter().map(|(c, _)| *c)).collect();
            used.sort();
            used.dedup();
            if used.len() >= 2 {
                let cols: Vec<(f32, f32)> = used.iter().map(|&c| g.bands[c]).collect();
                for r in run.iter_mut() {
                    for (c, _) in r.cells.iter_mut() {
                        *c = used.iter().position(|u| u == c).unwrap_or(0);
                    }
                }
                out.push((std::mem::take(run), cols));
            }
        }
        run.clear();
    };
    for r in g.rows {
        if tabular(&r) {
            run.push(r);
        } else {
            flush(&mut run);
        }
    }
    flush(&mut run);
    out
}

/// A borderless Word table: columns at their PDF positions, numbers right-aligned.
fn table_xml(rows: &[Row], cols: &[(f32, f32)], left: f32, max_w: f32) -> String {
    // Each column runs to the start of the next one.
    let mut widths: Vec<f32> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| match cols.get(i + 1) {
            Some(n) => n.0 - c.0,
            None => c.1 - c.0 + 6.0,
        })
        .collect();
    let indent = (cols[0].0 - left).max(0.0);
    let total: f32 = widths.iter().sum();
    let k = ((max_w - indent) / total).min(1.0);
    widths.iter_mut().for_each(|w| *w *= k);
    let tw = |v: f32| (v * TW).round() as i32;
    let mut x = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblInd w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/>\
<w:tblCellMar><w:left w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"0\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>",
        tw(widths.iter().sum()),
        tw(indent)
    );
    for w in &widths {
        x.push_str(&format!("<w:gridCol w:w=\"{}\"/>", tw(*w)));
    }
    x.push_str("</w:tblGrid>");
    for (i, r) in rows.iter().enumerate() {
        let pitch = rows.get(i + 1).map(|n| r.top - n.top).unwrap_or(r.top - r.bottom);
        x.push_str(&format!("<w:tr><w:trPr><w:trHeight w:val=\"{}\" w:hRule=\"atLeast\"/></w:trPr>", tw(pitch.max(1.0))));
        for (c, w) in widths.iter().enumerate() {
            x.push_str(&format!("<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/>", tw(*w)));
            match r.cells.iter().find(|(k, _)| *k == c) {
                Some((_, cell)) => {
                    // Right-aligned when the text ends near the column's right edge.
                    let right = number(&cell.text).is_some() || (cols[c].1 - cell.right).abs() < 2.0 && cell.left - cols[c].0 > 4.0;
                    if right {
                        x.push_str("<w:jc w:val=\"right\"/>");
                    }
                    x.push_str(&format!(
                        "</w:pPr><w:r><w:rPr>{}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:tc>",
                        run_props(&cell.font, cell.size, cell.color),
                        xml(&cell.text)
                    ));
                }
                None => x.push_str("</w:pPr></w:p></w:tc>"),
            }
        }
        x.push_str("</w:tr>");
    }
    x.push_str("</w:tbl>");
    x
}

fn bold(font: &str) -> bool {
    weight_from_name(font) >= 600
}

/// An Excel workbook with one sheet per page; text lined up in columns
/// (tables) lands in separate cells, and numbers become real numbers.
pub fn to_xlsx(bytes: &[u8], pages: &[u32], mut progress: impl FnMut(usize, usize)) -> Result<Vec<u8>> {
    let doc = open(bytes)?;
    let mut wb = Workbook::new();
    for (n, &p) in pages.iter().enumerate() {
        let page = get(&doc, p)?;
        let g = grid(cells(&page));
        let ws = wb.add_worksheet();
        ws.set_name(format!("Page {}", p + 1)).map_err(err)?;
        let mut widths: Vec<usize> = vec![];
        for (r, row) in g.rows.iter().enumerate() {
            for (c, cell) in &row.cells {
                let (r, c16) = (r as u32, *c as u16);
                let mut f = Format::new();
                if bold(&cell.font) {
                    f = f.set_bold();
                }
                match number(&cell.text) {
                    Some((v, d)) => {
                        let nf = if d == 0 { "#,##0".to_string() } else { format!("#,##0.{}", "0".repeat(d.min(6))) };
                        ws.write_number_with_format(r, c16, v, &f.set_num_format(nf)).map_err(err)?
                    }
                    None => ws.write_string_with_format(r, c16, &cell.text, &f).map_err(err)?,
                };
                if row.cells.len() > 1 {
                    if widths.len() <= *c {
                        widths.resize(c + 1, 0);
                    }
                    widths[*c] = widths[*c].max(cell.text.chars().count());
                }
            }
        }
        for (c, w) in widths.iter().enumerate() {
            ws.set_column_width(c as u16, (*w as f64 * 1.1 + 2.0).clamp(8.0, 60.0)).map_err(err)?;
        }
        progress(n + 1, pages.len());
    }
    if pages.is_empty() {
        wb.add_worksheet();
    }
    wb.save_to_buffer().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn numbers() {
        assert_eq!(number("1 250,50"), Some((1250.5, 2)));
        assert_eq!(number("1'250.50"), Some((1250.5, 2)));
        assert_eq!(number("1,250.50"), Some((1250.5, 2)));
        assert_eq!(number("1.250,50"), Some((1250.5, 2)));
        assert_eq!(number("CHF 12,5"), Some((12.5, 1)));
        assert_eq!(number("-3"), Some((-3.0, 0)));
        assert_eq!(number("(40)"), Some((-40.0, 0)));
        assert_eq!(number("0,75"), Some((0.75, 2)));
        assert_eq!(number("022 123 45 67"), None);
        assert_eq!(number("2026-114"), None);
        assert_eq!(number("12.05.2026"), None);
        assert_eq!(number("0123"), None);
        assert_eq!(number("abc"), None);
    }
}
