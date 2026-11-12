//! Annotations (highlight, underline, strike-out, notes, text boxes, shapes,
//! freehand drawing, stamps), written as standard PDF annotations with their
//! own appearance so that every PDF reader shows them the same way.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use serde::{Deserialize, Serialize};

use crate::std14::{text_width, winansi, wrap};
use crate::{KbError, Result};

type Rect = [f32; 4]; // left, bottom, right, top
type Rgb = [u8; 3];

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewAnnot {
    /// `rects`: one rectangle per line of text.
    Highlight { rects: Vec<Rect>, color: Rgb },
    Underline { rects: Vec<Rect>, color: Rgb },
    StrikeOut { rects: Vec<Rect>, color: Rgb },
    /// Sticky note at (x, y) (top-left corner of the icon).
    Note { x: f32, y: f32, text: String, color: Rgb },
    /// Text box.
    FreeText { rect: Rect, text: String, size: f32, color: Rgb, border: bool },
    Square { rect: Rect, color: Rgb, width: f32, fill: Option<Rgb> },
    Circle { rect: Rect, color: Rgb, width: f32, fill: Option<Rgb> },
    Line { from: [f32; 2], to: [f32; 2], color: Rgb, width: f32, arrow: bool },
    /// Freehand drawing: one list of points per stroke.
    Ink { strokes: Vec<Vec<[f32; 2]>>, color: Rgb, width: f32 },
    /// Rubber stamp ("APPROUVÉ", "PAYÉ"…) fitted in `rect`.
    Stamp { rect: Rect, label: String, color: Rgb },
}

#[derive(Debug, Clone, Serialize)]
pub struct AnnotInfo {
    /// Position in the page's annotation list (for deletion).
    pub index: usize,
    pub subtype: String,
    pub rect: Rect,
    pub contents: String,
    pub color: Option<Rgb>,
}

/// Annotation types the user can see and remove in the app (not links or form fields).
const MARKUP: &[&str] = &[
    "Text", "FreeText", "Line", "Square", "Circle", "Polygon", "PolyLine", "Highlight", "Underline", "Squiggly",
    "StrikeOut", "Stamp", "Caret", "Ink", "FileAttachment",
];

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

fn load(bytes: &[u8]) -> Result<Document> {
    let doc = Document::load_mem(bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err(err("document protégé par mot de passe"));
    }
    Ok(doc)
}

fn save(mut doc: Document) -> Result<Vec<u8>> {
    let mut out = vec![];
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}

fn page_id(doc: &Document, page_index: u32) -> Result<ObjectId> {
    doc.get_pages().get(&(page_index + 1)).copied().ok_or(KbError::BadPage(page_index + 1))
}

/// The page's /Annots array (resolved), with where it lives.
fn annots(doc: &Document, page: ObjectId) -> Result<Vec<Object>> {
    let dict = doc.get_dictionary(page).map_err(err)?;
    Ok(match dict.get(b"Annots") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(Object::Reference(r)) => doc.get_object(*r).and_then(|o| o.as_array().cloned()).unwrap_or_default(),
        _ => vec![],
    })
}

fn set_annots(doc: &mut Document, page: ObjectId, list: Vec<Object>) -> Result<()> {
    let dict = doc.get_object_mut(page).and_then(|o| o.as_dict_mut()).map_err(err)?;
    dict.set("Annots", Object::Array(list));
    Ok(())
}

fn nums(v: &[f32]) -> Object {
    Object::Array(v.iter().map(|&n| Object::Real(n)).collect())
}

fn rgb(c: Rgb) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

fn bounds(points: impl Iterator<Item = [f32; 2]>, pad: f32) -> Rect {
    let mut r = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for p in points {
        r = [r[0].min(p[0]), r[1].min(p[1]), r[2].max(p[0]), r[3].max(p[1])];
    }
    [r[0] - pad, r[1] - pad, r[2] + pad, r[3] + pad]
}

fn union(rects: &[Rect]) -> Rect {
    bounds(rects.iter().flat_map(|r| [[r[0], r[1]], [r[2], r[3]]]), 0.0)
}

fn pdf_date() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    // Civil date from days since epoch (UTC).
    let days = (now / 86_400) as i64;
    let secs = now % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("D:{y:04}{m:02}{d:02}{:02}{:02}{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

fn text(s: &str) -> Object {
    // PDF text strings: UTF-16BE with BOM handles every language.
    let mut v = vec![0xFE, 0xFF];
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_be_bytes());
    }
    Object::String(v, StringFormat::Hexadecimal)
}

fn pdf_str(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            32..=126 => s.push(b as char),
            _ => s.push_str(&format!("\\{b:03o}")),
        }
    }
    s.push(')');
    s
}

/// Appearance stream drawn in page coordinates over `bbox`.
fn appearance(doc: &mut Document, bbox: Rect, content: String, opacity: Option<f32>, multiply: bool, font: bool) -> ObjectId {
    let mut res = Dictionary::new();
    if opacity.is_some() || multiply {
        let mut gs = dictionary! { "Type" => "ExtGState" };
        if let Some(a) = opacity {
            gs.set("CA", Object::Real(a));
            gs.set("ca", Object::Real(a));
        }
        if multiply {
            gs.set("BM", "Multiply");
        }
        res.set("ExtGState", dictionary! { "GS0" => gs });
    }
    if font {
        res.set(
            "Font",
            dictionary! {
                "Helv" => dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" },
                "HeBo" => dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding" },
            },
        );
    }
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => nums(&bbox),
        "Resources" => res,
    };
    doc.add_object(Stream::new(dict, content.into_bytes()))
}

fn circle_path(r: Rect) -> String {
    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    let (rx, ry) = ((r[2] - r[0]) / 2.0, (r[3] - r[1]) / 2.0);
    let k = 0.552_284_8;
    format!(
        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h",
        cx + rx, cy,
        cx + rx, cy + k * ry, cx + k * rx, cy + ry, cx, cy + ry,
        cx - k * rx, cy + ry, cx - rx, cy + k * ry, cx - rx, cy,
        cx - rx, cy - k * ry, cx - k * rx, cy - ry, cx, cy - ry,
        cx + k * rx, cy - ry, cx + rx, cy - k * ry, cx + rx, cy,
    )
}

/// Adds an annotation to a page (0-based).
pub fn add_annotation(bytes: &[u8], page_index: u32, a: &NewAnnot) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let page = page_id(&doc, page_index)?;

    let mut d = dictionary! {
        "Type" => "Annot",
        "F" => 4, // printed
        "P" => page,
        "M" => Object::string_literal(pdf_date()),
        "CreationDate" => Object::string_literal(pdf_date()),
        "NM" => Object::string_literal(format!("kb-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())),
        "T" => text("KB PDF Reader"),
    };
    let set_color = |d: &mut Dictionary, c: Rgb| d.set("C", nums(&rgb(c)));
    let (rect, ap) = match a {
        NewAnnot::Highlight { rects, color } | NewAnnot::Underline { rects, color } | NewAnnot::StrikeOut { rects, color } => {
            if rects.is_empty() {
                return Err(err("aucune zone de texte"));
            }
            let sub = match a {
                NewAnnot::Highlight { .. } => "Highlight",
                NewAnnot::Underline { .. } => "Underline",
                _ => "StrikeOut",
            };
            d.set("Subtype", sub);
            set_color(&mut d, *color);
            let quads: Vec<f32> = rects.iter().flat_map(|r| [r[0], r[3], r[2], r[3], r[0], r[1], r[2], r[1]]).collect();
            d.set("QuadPoints", nums(&quads));
            let [cr, cg, cb] = rgb(*color);
            let mut c = format!("q {cr} {cg} {cb} rg {cr} {cg} {cb} RG ");
            for r in rects {
                let h = r[3] - r[1];
                match a {
                    NewAnnot::Highlight { .. } => c += &format!("/GS0 gs {} {} {} {} re f ", r[0], r[1], r[2] - r[0], h),
                    NewAnnot::Underline { .. } => {
                        let w = (h * 0.07).max(0.6);
                        c += &format!("{w} w {} {} m {} {} l S ", r[0], r[1] + h * 0.08, r[2], r[1] + h * 0.08)
                    }
                    _ => {
                        let w = (h * 0.07).max(0.6);
                        c += &format!("{w} w {} {} m {} {} l S ", r[0], r[1] + h * 0.45, r[2], r[1] + h * 0.45)
                    }
                }
            }
            c += "Q";
            let bbox = union(rects);
            let multiply = matches!(a, NewAnnot::Highlight { .. });
            let ap = appearance(&mut doc, bbox, c, None, multiply, false);
            (bbox, ap)
        }
        NewAnnot::Note { x, y, text: t, color } => {
            d.set("Subtype", "Text");
            d.set("Name", "Comment");
            d.set("Contents", text(t));
            d.set("Open", false);
            set_color(&mut d, *color);
            let r = [*x, *y - 20.0, *x + 20.0, *y];
            let [cr, cg, cb] = rgb(*color);
            let c = format!(
                "q {cr} {cg} {cb} rg 0.25 0.25 0.25 RG 0.8 w {l} {b} m {r1} {b} l {r} {f} l {r} {t} l {l} {t} l h B \
                 {r1} {b} m {r1} {f} l {r} {f} l S \
                 0.35 0.35 0.35 RG 1 w {a} {l1} m {e} {l1} l {a} {l2} m {e} {l2} l {a} {l3} m {g} {l3} l S Q",
                l = r[0] + 1.0, b = r[1] + 1.0, r = r[2] - 1.0, t = r[3] - 1.0, r1 = r[2] - 6.0, f = r[1] + 6.0,
                a = r[0] + 4.0, e = r[2] - 4.0, g = r[2] - 8.0, l1 = r[3] - 5.0, l2 = r[3] - 9.0, l3 = r[3] - 13.0,
            );
            let ap = appearance(&mut doc, r, c, None, false, false);
            (r, ap)
        }
        NewAnnot::FreeText { rect, text: t, size, color, border } => {
            d.set("Subtype", "FreeText");
            d.set("Contents", text(t));
            let [cr, cg, cb] = rgb(*color);
            d.set("DA", Object::string_literal(format!("{cr} {cg} {cb} rg /Helv {size} Tf")));
            d.set("BS", dictionary! { "W" => if *border { 1 } else { 0 } });
            let pad = 3.0;
            let lines = wrap(t, *size, false, rect[2] - rect[0] - 2.0 * pad);
            // Grow downwards if the text needs more room.
            let needed = lines.len() as f32 * size * 1.2 + 2.0 * pad;
            let r = [rect[0], rect[1].min(rect[3] - needed), rect[2], rect[3]];
            let mut c = String::from("q ");
            if *border {
                c += &format!("{cr} {cg} {cb} RG 1 w {} {} {} {} re S ", r[0] + 0.5, r[1] + 0.5, r[2] - r[0] - 1.0, r[3] - r[1] - 1.0);
            }
            c += &format!("BT {cr} {cg} {cb} rg /Helv {size} Tf ");
            for (n, line) in lines.iter().enumerate() {
                let y = r[3] - pad - size * 0.9 - n as f32 * size * 1.2;
                c += &format!("1 0 0 1 {} {y} Tm {} Tj ", r[0] + pad, pdf_str(&winansi(line)));
            }
            c += "ET Q";
            let ap = appearance(&mut doc, r, c, None, false, true);
            (r, ap)
        }
        NewAnnot::Square { rect, color, width, fill } | NewAnnot::Circle { rect, color, width, fill } => {
            let circle = matches!(a, NewAnnot::Circle { .. });
            d.set("Subtype", if circle { "Circle" } else { "Square" });
            set_color(&mut d, *color);
            d.set("BS", dictionary! { "W" => Object::Real(*width) });
            let [cr, cg, cb] = rgb(*color);
            let mut c = format!("q {cr} {cg} {cb} RG {width} w ");
            if let Some(f) = fill {
                d.set("IC", nums(&rgb(*f)));
                let [fr, fg, fb] = rgb(*f);
                c += &format!("{fr} {fg} {fb} rg ");
            }
            let h = width / 2.0;
            let inner = [rect[0] + h, rect[1] + h, rect[2] - h, rect[3] - h];
            if circle {
                c += &circle_path(inner);
            } else {
                c += &format!("{} {} {} {} re", inner[0], inner[1], inner[2] - inner[0], inner[3] - inner[1]);
            }
            c += if fill.is_some() { " B Q" } else { " S Q" };
            let ap = appearance(&mut doc, *rect, c, None, false, false);
            (*rect, ap)
        }
        NewAnnot::Line { from, to, color, width, arrow } => {
            d.set("Subtype", "Line");
            set_color(&mut d, *color);
            d.set("L", nums(&[from[0], from[1], to[0], to[1]]));
            d.set("BS", dictionary! { "W" => Object::Real(*width) });
            if *arrow {
                d.set("LE", Object::Array(vec!["None".into(), "OpenArrow".into()]));
            }
            let [cr, cg, cb] = rgb(*color);
            let mut c = format!("q {cr} {cg} {cb} RG {width} w 1 J 1 j {} {} m {} {} l S ", from[0], from[1], to[0], to[1]);
            let len = 4.0 + width * 3.0;
            if *arrow {
                let ang = (to[1] - from[1]).atan2(to[0] - from[0]);
                for s in [-0.45_f32, 0.45] {
                    let (x, y) = (to[0] - len * (ang + s).cos(), to[1] - len * (ang + s).sin());
                    c += &format!("{x} {y} m {} {} l S ", to[0], to[1]);
                }
            }
            c += "Q";
            let r = bounds([*from, *to].into_iter(), len + width);
            let ap = appearance(&mut doc, r, c, None, false, false);
            (r, ap)
        }
        NewAnnot::Ink { strokes, color, width } => {
            let strokes: Vec<&Vec<[f32; 2]>> = strokes.iter().filter(|s| !s.is_empty()).collect();
            if strokes.is_empty() {
                return Err(err("dessin vide"));
            }
            d.set("Subtype", "Ink");
            set_color(&mut d, *color);
            d.set("BS", dictionary! { "W" => Object::Real(*width) });
            d.set(
                "InkList",
                Object::Array(strokes.iter().map(|s| nums(&s.iter().flat_map(|p| [p[0], p[1]]).collect::<Vec<_>>())).collect()),
            );
            let [cr, cg, cb] = rgb(*color);
            let mut c = format!("q {cr} {cg} {cb} RG {width} w 1 J 1 j ");
            for s in &strokes {
                c += &format!("{} {} m ", s[0][0], s[0][1]);
                if s.len() == 1 {
                    c += &format!("{} {} l ", s[0][0] + 0.01, s[0][1]);
                }
                for p in &s[1..] {
                    c += &format!("{} {} l ", p[0], p[1]);
                }
                c += "S ";
            }
            c += "Q";
            let r = bounds(strokes.iter().flat_map(|s| s.iter().copied()), width + 1.0);
            let ap = appearance(&mut doc, r, c, None, false, false);
            (r, ap)
        }
        NewAnnot::Stamp { rect, label, color } => {
            d.set("Subtype", "Stamp");
            d.set("Name", "Draft");
            d.set("Contents", text(label));
            set_color(&mut d, *color);
            let enc = winansi(&label.to_uppercase());
            let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
            let size = (h * 0.55).min((w - 12.0) / (text_width(&enc, 1.0, true)).max(0.01)).max(4.0);
            let tw = text_width(&enc, size, true);
            let [cr, cg, cb] = rgb(*color);
            let c = format!(
                "q /GS0 gs {cr} {cg} {cb} RG {cr} {cg} {cb} rg 2.5 w {} {} {} {} re S \
                 BT /HeBo {size} Tf 1 0 0 1 {} {} Tm {} Tj ET Q",
                rect[0] + 1.25, rect[1] + 1.25, w - 2.5, h - 2.5,
                rect[0] + (w - tw) / 2.0, rect[1] + (h - size * 0.7) / 2.0, pdf_str(&enc),
            );
            let ap = appearance(&mut doc, *rect, c, Some(0.85), false, true);
            (*rect, ap)
        }
    };
    d.set("Rect", nums(&rect));
    d.set("AP", dictionary! { "N" => ap });
    let id = doc.add_object(d);
    let mut list = annots(&doc, page)?;
    list.push(Object::Reference(id));
    set_annots(&mut doc, page, list)?;
    save(doc)
}

fn resolve<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a Dictionary> {
    match o {
        Object::Reference(r) => doc.get_dictionary(*r).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

fn decode_text(o: Option<&Object>) -> String {
    match o {
        Some(Object::String(b, _)) => {
            if b.starts_with(&[0xFE, 0xFF]) {
                let u: Vec<u16> = b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&u)
            } else {
                b.iter().map(|&c| c as char).collect()
            }
        }
        _ => String::new(),
    }
}

/// Annotations of a page that the user can see and remove.
pub fn list_annotations(bytes: &[u8], page_index: u32) -> Result<Vec<AnnotInfo>> {
    let doc = Document::load_mem(bytes).map_err(err)?;
    let page = page_id(&doc, page_index)?;
    let mut out = vec![];
    for (index, o) in annots(&doc, page)?.iter().enumerate() {
        let Some(d) = resolve(&doc, o) else { continue };
        let subtype = d.get(b"Subtype").and_then(|s| s.as_name()).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
        if !MARKUP.contains(&subtype.as_str()) {
            continue;
        }
        let f = |o: &Object| o.as_float().ok().or_else(|| o.as_i64().ok().map(|i| i as f32));
        let rect = d
            .get(b"Rect")
            .and_then(|r| r.as_array())
            .ok()
            .and_then(|a| (a.len() == 4).then(|| [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])]))
            .and_then(|r| Some([r[0]?, r[1]?, r[2]?, r[3]?]))
            .map(|r| [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])])
            .unwrap_or([0.0; 4]);
        let color = d.get(b"C").and_then(|c| c.as_array()).ok().and_then(|a| {
            (a.len() == 3).then(|| {
                let c = |o: &Object| (f(o).unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                [c(&a[0]), c(&a[1]), c(&a[2])]
            })
        });
        out.push(AnnotInfo { index, subtype, rect, contents: decode_text(d.get(b"Contents").ok()), color });
    }
    Ok(out)
}

/// Removes an annotation (by its `AnnotInfo::index`) and its pop-up, if any.
pub fn delete_annotation(bytes: &[u8], page_index: u32, index: usize) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let page = page_id(&doc, page_index)?;
    let mut list = annots(&doc, page)?;
    if index >= list.len() {
        return Err(err("annotation introuvable"));
    }
    let removed = list.remove(index);
    let popup = resolve(&doc, &removed).and_then(|d| d.get(b"Popup").ok()).and_then(|p| p.as_reference().ok());
    if let Some(p) = popup {
        list.retain(|o| o.as_reference().ok() != Some(p));
    }
    set_annots(&mut doc, page, list)?;
    doc.prune_objects();
    save(doc)
}

/// Changes the text of a note.
pub fn set_note_text(bytes: &[u8], page_index: u32, index: usize, new_text: &str) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let page = page_id(&doc, page_index)?;
    let list = annots(&doc, page)?;
    let id = list.get(index).and_then(|o| o.as_reference().ok()).ok_or_else(|| err("annotation introuvable"))?;
    let d = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(err)?;
    d.set("Contents", text(new_text));
    d.set("M", Object::string_literal(pdf_date()));
    save(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_format() {
        let d = pdf_date();
        assert!(d.starts_with("D:20") && d.len() == 17, "{d}");
    }

    #[test]
    fn escapes_strings() {
        assert_eq!(pdf_str(b"a(b)\\c\xe9"), "(a\\(b\\)\\\\c\\351)");
    }
}
