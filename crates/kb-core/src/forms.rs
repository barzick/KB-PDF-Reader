//! Interactive forms (AcroForm): listing fields, filling them and writing
//! appearance streams so every viewer shows the same values.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::annot::{decode_text, err, load, nums, pdf_str, resolve, save, text};
use crate::std14::{text_width, winansi, wrap};
use crate::annot::Rect;
use crate::Result;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Checkbox,
    Radio,
    Combo,
    List,
    Button,
    Signature,
}

/// One widget of a form field (a radio group has one widget per choice).
#[derive(Debug, Clone, Serialize)]
pub struct FormField {
    /// Fully qualified field name.
    pub name: String,
    pub kind: FieldKind,
    pub page: u32,
    pub rect: Rect,
    /// Text/choice value, or the selected state name for buttons.
    pub value: String,
    /// Checkbox/radio: this widget's "on" state name.
    pub export: String,
    pub checked: bool,
    pub options: Vec<String>,
    pub multiline: bool,
    pub readonly: bool,
    pub max_len: Option<u32>,
    pub font_size: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldValue {
    pub name: String,
    /// Text/choice: the text. Checkbox: "true"/"false". Radio: the export name.
    pub value: String,
}

const READONLY: i64 = 1;
const MULTILINE: i64 = 1 << 12;
const RADIO: i64 = 1 << 15;
const PUSHBUTTON: i64 = 1 << 16;
const COMBO: i64 = 1 << 17;

fn dict_of(doc: &Document, id: ObjectId) -> Option<&Dictionary> {
    doc.get_dictionary(id).ok()
}

fn parent(d: &Dictionary) -> Option<ObjectId> {
    d.get(b"Parent").ok().and_then(|o| o.as_reference().ok())
}

/// An inheritable field attribute, looked up the /Parent chain.
fn inherited<'a>(doc: &'a Document, mut d: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    for _ in 0..32 {
        if let Ok(v) = d.get(key) {
            return Some(match v {
                Object::Reference(r) => doc.get_object(*r).ok()?,
                v => v,
            });
        }
        d = dict_of(doc, parent(d)?)?;
    }
    None
}

/// The field dictionary a widget belongs to (the nearest one with /T).
fn field_id(doc: &Document, widget: ObjectId) -> ObjectId {
    let mut id = widget;
    for _ in 0..32 {
        let Some(d) = dict_of(doc, id) else { break };
        if d.has(b"T") {
            return id;
        }
        match parent(d) {
            Some(p) => id = p,
            None => break,
        }
    }
    widget
}

fn full_name(doc: &Document, widget: ObjectId) -> String {
    let mut parts = vec![];
    let mut id = Some(widget);
    for _ in 0..32 {
        let Some(d) = id.and_then(|i| dict_of(doc, i)) else { break };
        if let Ok(t) = d.get(b"T") {
            parts.push(decode_text(Some(t)));
        }
        id = parent(d);
    }
    parts.reverse();
    parts.join(".")
}

fn name_of(o: Option<&Object>) -> String {
    match o {
        Some(Object::Name(n)) => String::from_utf8_lossy(n).into_owned(),
        Some(o @ Object::String(..)) => decode_text(Some(o)),
        _ => String::new(),
    }
}

/// The "on" appearance state of a checkbox or radio widget.
fn on_state(doc: &Document, w: &Dictionary) -> String {
    let n = w
        .get(b"AP")
        .ok()
        .and_then(|o| resolve(doc, o))
        .and_then(|ap| ap.get(b"N").ok())
        .and_then(|o| resolve(doc, o));
    n.and_then(|n| n.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).find(|k| k != "Off"))
        .unwrap_or_else(|| "Yes".into())
}

fn rect_of(doc: &Document, w: &Dictionary) -> Rect {
    let mut r = [0.0; 4];
    if let Some(Object::Array(a)) = w.get(b"Rect").ok().map(|o| match o {
        Object::Reference(id) => doc.get_object(*id).unwrap_or(o),
        o => o,
    }) {
        for (i, v) in a.iter().take(4).enumerate() {
            r[i] = v.as_float().unwrap_or(0.0);
        }
    }
    [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])]
}

fn options(doc: &Document, d: &Dictionary) -> Vec<String> {
    match inherited(doc, d, b"Opt") {
        Some(Object::Array(a)) => a
            .iter()
            .map(|o| match o {
                Object::Array(pair) => decode_text(pair.get(1).or(pair.first())),
                Object::Reference(r) => decode_text(doc.get_object(*r).ok()),
                o => decode_text(Some(o)),
            })
            .collect(),
        _ => vec![],
    }
}

/// Font size from a default appearance string such as "/Helv 0 Tf 0 g".
fn da_size(da: &str) -> f32 {
    let toks: Vec<&str> = da.split_whitespace().collect();
    toks.iter().position(|t| *t == "Tf").and_then(|i| i.checked_sub(1)).and_then(|i| toks[i].parse().ok()).unwrap_or(0.0)
}

/// Colour operator from a default appearance string ("0 g", "1 0 0 rg"...).
fn da_color(da: &str) -> String {
    let toks: Vec<&str> = da.split_whitespace().collect();
    for (i, t) in toks.iter().enumerate() {
        let n = match *t {
            "g" => 1,
            "rg" => 3,
            "k" => 4,
            _ => continue,
        };
        if i >= n {
            return toks[i - n..=i].join(" ");
        }
    }
    "0 g".into()
}

fn widgets(doc: &Document) -> Vec<(u32, ObjectId)> {
    let mut out = vec![];
    for (n, (_, page)) in doc.get_pages().into_iter().enumerate() {
        let Ok(pd) = doc.get_dictionary(page) else { continue };
        let list = match pd.get(b"Annots") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(r)) => doc.get_object(*r).and_then(|o| o.as_array().cloned()).unwrap_or_default(),
            _ => vec![],
        };
        for o in list {
            let Ok(id) = o.as_reference() else { continue };
            if dict_of(doc, id).is_some_and(|d| d.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(b"Widget")) {
                out.push((n as u32, id));
            }
        }
    }
    out
}

fn describe(doc: &Document, page: u32, wid: ObjectId) -> Option<FormField> {
    let w = dict_of(doc, wid)?;
    let ft = name_of(inherited(doc, w, b"FT"));
    let ff = inherited(doc, w, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    let kind = match ft.as_str() {
        "Tx" => FieldKind::Text,
        "Btn" if ff & PUSHBUTTON != 0 => FieldKind::Button,
        "Btn" if ff & RADIO != 0 => FieldKind::Radio,
        "Btn" => FieldKind::Checkbox,
        "Ch" if ff & COMBO != 0 => FieldKind::Combo,
        "Ch" => FieldKind::List,
        "Sig" => FieldKind::Signature,
        _ => return None,
    };
    let v = inherited(doc, w, b"V");
    let value = match v {
        Some(Object::Array(a)) => a.first().map(|o| decode_text(Some(o))).unwrap_or_default(),
        v => name_of(v),
    };
    let export = on_state(doc, w);
    let checked = match kind {
        FieldKind::Checkbox | FieldKind::Radio => w.get(b"AS").ok().map(|o| name_of(Some(o))).is_some_and(|s| s != "Off" && s == export),
        _ => false,
    };
    let da = inherited(doc, w, b"DA").map(|o| decode_text(Some(o))).unwrap_or_default();
    Some(FormField {
        name: full_name(doc, wid),
        kind,
        page,
        rect: rect_of(doc, w),
        value,
        export,
        checked,
        options: options(doc, w),
        multiline: ff & MULTILINE != 0,
        readonly: ff & READONLY != 0,
        max_len: inherited(doc, w, b"MaxLen").and_then(|o| o.as_i64().ok()).map(|n| n as u32),
        font_size: da_size(&da),
    })
}

/// Every form field widget of the document, in page order.
pub fn list_fields(bytes: &[u8]) -> Result<Vec<FormField>> {
    let doc = load(bytes)?;
    Ok(widgets(&doc).into_iter().filter_map(|(p, w)| describe(&doc, p, w)).collect())
}

/// Appearance for a text or combo widget showing `value`.
fn text_appearance(doc: &mut Document, wid: ObjectId, f: &FormField, value: &str) -> Result<ObjectId> {
    let w = dict_of(doc, wid).ok_or_else(|| err("champ introuvable"))?;
    let da = inherited(doc, w, b"DA").map(|o| decode_text(Some(o))).unwrap_or_default();
    let q = inherited(doc, w, b"Q").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    let color = da_color(&da);
    let (bw, bh) = (f.rect[2] - f.rect[0], f.rect[3] - f.rect[1]);
    let pad = 2.0;
    let avail = (bw - 2.0 * pad).max(1.0);

    let lines: Vec<String> = if f.multiline {
        value.split('\n').flat_map(|para| {
            let size = if f.font_size > 0.0 { f.font_size } else { 10.0 };
            let l = wrap(para, size, false, avail);
            if l.is_empty() { vec![String::new()] } else { l }
        }).collect()
    } else {
        vec![value.replace('\n', " ")]
    };
    let size = if f.font_size > 0.0 {
        f.font_size
    } else if f.multiline {
        10.0
    } else {
        // Auto size: as big as fits the height and the width.
        let tw = text_width(&winansi(&lines[0]), 1.0, false).max(0.01);
        (bh * 0.7).min(avail / tw).clamp(4.0, 12.0)
    };

    // Background and border from the widget's appearance characteristics.
    let mut c = String::new();
    let mk = w.get(b"MK").ok().and_then(|o| resolve(doc, o));
    let colour = |key: &[u8], fill: bool| -> Option<String> {
        let Ok(Object::Array(a)) = mk?.get(key) else { return None };
        let v: Vec<String> = a.iter().filter_map(|o| o.as_float().ok()).map(|f| f.to_string()).collect();
        let op = match (v.len(), fill) {
            (1, true) => "g",
            (1, false) => "G",
            (3, true) => "rg",
            (3, false) => "RG",
            (4, true) => "k",
            (4, false) => "K",
            _ => return None,
        };
        Some(format!("{} {op}", v.join(" ")))
    };
    if let Some(bg) = colour(b"BG", true) {
        c += &format!("q {bg} 0 0 {bw} {bh} re f Q ");
    }
    if let Some(bc) = colour(b"BC", false) {
        let bwid = w
            .get(b"BS")
            .ok()
            .and_then(|o| resolve(doc, o))
            .and_then(|bs| bs.get(b"W").ok())
            .and_then(|o| o.as_float().ok())
            .unwrap_or(1.0);
        if bwid > 0.0 {
            c += &format!("q {bc} {bwid} w {h} {h} {} {} re S Q ", bw - bwid, bh - bwid, h = bwid / 2.0);
        }
    }
    c += "/Tx BMC q ";
    c += &format!("{pad} {pad} {} {} re W n BT /KBHelv {size} Tf {color} ", bw - 2.0 * pad, bh - 2.0 * pad);
    for (n, line) in lines.iter().enumerate() {
        let enc = winansi(line);
        let tw = text_width(&enc, size, false);
        let x = match q {
            1 => (bw - tw) / 2.0,
            2 => bw - pad - tw,
            _ => pad,
        };
        let y = if f.multiline { bh - pad - size * 0.9 - n as f32 * size * 1.15 } else { (bh - size * 0.72) / 2.0 };
        c += &format!("1 0 0 1 {x} {y} Tm {} Tj ", pdf_str(&enc));
    }
    c += "ET Q EMC";
    let dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "BBox" => nums(&[0.0, 0.0, bw, bh]),
        "Resources" => dictionary! {
            "Font" => dictionary! {
                "KBHelv" => dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" },
            },
        },
    };
    Ok(doc.add_object(Stream::new(dict, c.into_bytes())))
}

fn set(doc: &mut Document, id: ObjectId, key: &str, v: Object) -> Result<()> {
    doc.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(err)?.set(key, v);
    Ok(())
}

/// Sets field values and rebuilds their appearances.
pub fn fill_fields(bytes: &[u8], values: &[FieldValue]) -> Result<Vec<u8>> {
    let mut doc = load(bytes)?;
    let all: Vec<(ObjectId, FormField)> =
        widgets(&doc).into_iter().filter_map(|(p, w)| describe(&doc, p, w).map(|f| (w, f))).collect();
    for v in values {
        let targets: Vec<&(ObjectId, FormField)> = all.iter().filter(|(_, f)| f.name == v.name).collect();
        if targets.is_empty() {
            return Err(err(format!("champ « {} » introuvable", v.name)));
        }
        for (wid, f) in targets {
            if f.readonly {
                continue;
            }
            let field = field_id(&doc, *wid);
            match f.kind {
                FieldKind::Text | FieldKind::Combo | FieldKind::List => {
                    let mut value = v.value.clone();
                    if let Some(max) = f.max_len {
                        value = value.chars().take(max as usize).collect();
                    }
                    set(&mut doc, field, "V", text(&value))?;
                    if f.kind == FieldKind::List {
                        // Let viewers draw list boxes themselves from the selection.
                        if let Some(i) = f.options.iter().position(|o| *o == value) {
                            set(&mut doc, field, "I", Object::Array(vec![Object::Integer(i as i64)]))?;
                        }
                        continue;
                    }
                    let ap = text_appearance(&mut doc, *wid, f, &value)?;
                    set(&mut doc, *wid, "AP", Object::Dictionary(dictionary! { "N" => ap }))?;
                }
                FieldKind::Checkbox => {
                    let on = matches!(v.value.as_str(), "true" | "1" | "on" | "yes") || v.value == f.export;
                    let state = if on { f.export.as_str() } else { "Off" };
                    set(&mut doc, field, "V", Object::Name(state.as_bytes().to_vec()))?;
                    set(&mut doc, *wid, "AS", Object::Name(state.as_bytes().to_vec()))?;
                }
                FieldKind::Radio => {
                    let state = if v.value == f.export { f.export.as_str() } else { "Off" };
                    set(&mut doc, field, "V", Object::Name(v.value.as_bytes().to_vec()))?;
                    set(&mut doc, *wid, "AS", Object::Name(state.as_bytes().to_vec()))?;
                }
                FieldKind::Button | FieldKind::Signature => {}
            }
        }
    }
    // Our appearances are up to date: viewers must not regenerate them.
    if let Ok(af) = doc.catalog().and_then(|c| c.get(b"AcroForm")).cloned() {
        match af {
            Object::Reference(id) => {
                let _ = set(&mut doc, id, "NeedAppearances", Object::Boolean(false));
            }
            Object::Dictionary(mut d) => {
                d.set("NeedAppearances", false);
                let root = doc.trailer.get(b"Root").and_then(|o| o.as_reference()).map_err(err)?;
                set(&mut doc, root, "AcroForm", Object::Dictionary(d))?;
            }
            _ => {}
        }
    }
    save(doc)
}

/// Turns form fields and annotations into ordinary page content.
pub fn flatten(bytes: &[u8]) -> Result<Vec<u8>> {
    let pdfium = crate::pdfium()?;
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    for i in 0..doc.pages().len() {
        let mut page = doc.pages().get(i)?;
        page.flatten()?;
    }
    let out = doc.save_to_bytes()?;
    drop(doc);
    // Remove the now empty AcroForm so viewers stop offering form filling.
    let mut d = load(&out)?;
    let root = d.trailer.get(b"Root").and_then(|o| o.as_reference()).map_err(err)?;
    d.get_object_mut(root).and_then(|o| o.as_dict_mut()).map_err(err)?.remove(b"AcroForm");
    save(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_appearance_parsing() {
        assert_eq!(da_size("/Helv 0 Tf 0 g"), 0.0);
        assert_eq!(da_size("/F1 11.5 Tf 1 0 0 rg"), 11.5);
        assert_eq!(da_color("/F1 11.5 Tf 1 0 0 rg"), "1 0 0 rg");
        assert_eq!(da_color("/Helv 9 Tf"), "0 g");
    }
}
