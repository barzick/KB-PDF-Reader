//! Lossless text removal and overlay, working directly on the page's content stream.
//!
//! PDFium can rewrite a whole page, but doing so drops the fine spacing
//! (kerning) that Word/Acrobat put in the original. Instead we:
//! 1. find the exact text-showing operators of the edited paragraph in the
//!    original content stream, and replace each one by an invisible move of the
//!    same width (so any text that follows on the same line stays in place);
//! 2. add the new text as a separate drawing (Form XObject) on top.
//!
//! Everything else on the page is kept byte-for-byte equivalent.

use std::collections::{BTreeMap, HashSet};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{KbError, Result};

fn err(msg: impl Into<String>) -> KbError {
    KbError::Pdf(msg.into())
}

/// Glyph widths of one font, in 1/1000 of text space unit.
struct Widths {
    two_byte: bool,
    map: BTreeMap<u32, f32>,
    default: f32,
    /// Multiplier for Type3 fonts (FontMatrix), 1.0 otherwise.
    scale: f32,
}

impl Widths {
    fn codes<'a>(&'a self, s: &'a [u8]) -> impl Iterator<Item = u32> + 'a {
        let step = if self.two_byte { 2 } else { 1 };
        s.chunks(step).map(move |c| {
            if step == 2 && c.len() == 2 { ((c[0] as u32) << 8) | c[1] as u32 } else { c[0] as u32 }
        })
    }
    fn width(&self, code: u32) -> f32 {
        self.map.get(&code).copied().unwrap_or(self.default) * self.scale
    }
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

fn font_widths(doc: &Document, font: &Dictionary) -> Result<Widths> {
    let subtype = font.get(b"Subtype").and_then(Object::as_name).unwrap_or(b"");
    if subtype == b"Type0" {
        let enc = font.get(b"Encoding").map(|e| deref(doc, e));
        match enc {
            Ok(Object::Name(n)) if n == b"Identity-H" || n == b"Identity-V" => {}
            _ => return Err(err("encodage de police non pris en charge")),
        }
        let desc = font
            .get(b"DescendantFonts")
            .map(|d| deref(doc, d))
            .and_then(Object::as_array)
            .ok()
            .and_then(|a| a.first())
            .map(|d| deref(doc, d))
            .and_then(|d| d.as_dict().ok())
            .ok_or_else(|| err("police CID incomplète"))?;
        let default = desc.get(b"DW").ok().and_then(num).unwrap_or(1000.0);
        let mut map = BTreeMap::new();
        if let Ok(w) = desc.get(b"W").map(|w| deref(doc, w)).and_then(Object::as_array) {
            let mut i = 0;
            while i < w.len() {
                let Some(first) = num(deref(doc, &w[i])) else { break };
                match w.get(i + 1).map(|o| deref(doc, o)) {
                    Some(Object::Array(list)) => {
                        for (k, v) in list.iter().enumerate() {
                            if let Some(v) = num(deref(doc, v)) {
                                map.insert(first as u32 + k as u32, v);
                            }
                        }
                        i += 2;
                    }
                    Some(last) => {
                        let (Some(last), Some(v)) = (num(last), w.get(i + 2).and_then(|o| num(deref(doc, o)))) else { break };
                        for c in first as u32..=last as u32 {
                            map.insert(c, v);
                        }
                        i += 3;
                    }
                    None => break,
                }
            }
        }
        return Ok(Widths { two_byte: true, map, default, scale: 1.0 });
    }

    let widths = font.get(b"Widths").map(|w| deref(doc, w)).and_then(Object::as_array);
    let Ok(widths) = widths else {
        return Err(err("police sans table de largeurs"));
    };
    let first = font.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as u32;
    let missing = font
        .get(b"FontDescriptor")
        .map(|d| deref(doc, d))
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"MissingWidth").ok().and_then(num))
        .unwrap_or(0.0);
    let mut map = BTreeMap::new();
    for (k, w) in widths.iter().enumerate() {
        if let Some(w) = num(deref(doc, w)) {
            map.insert(first + k as u32, w);
        }
    }
    let scale = if subtype == b"Type3" {
        font.get(b"FontMatrix")
            .map(|m| deref(doc, m))
            .and_then(Object::as_array)
            .ok()
            .and_then(|m| m.first().and_then(num))
            .map(|a| a * 1000.0)
            .unwrap_or(1.0)
    } else {
        1.0
    };
    Ok(Widths { two_byte: false, map, default: missing, scale })
}

#[derive(Clone)]
struct TextState {
    font: Vec<u8>,
    size: f32,
    tc: f32,
    tw: f32,
}

fn is_show(op: &str) -> bool {
    matches!(op, "Tj" | "TJ" | "'" | "\"")
}

/// Strings shown by a text operator (empty strings excluded).
fn shown_strings(op: &Operation) -> Vec<&[u8]> {
    let strings: Vec<&[u8]> = match op.operator.as_str() {
        "Tj" | "'" => op.operands.first().and_then(|o| o.as_str().ok()).into_iter().collect(),
        "\"" => op.operands.get(2).and_then(|o| o.as_str().ok()).into_iter().collect(),
        "TJ" => op
            .operands
            .first()
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().filter_map(|o| o.as_str().ok()).collect())
            .unwrap_or_default(),
        _ => vec![],
    };
    strings.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Counts the text objects PDFium will see on the page (one per non-empty show operator).
fn text_ops(ops: &[Operation]) -> Vec<usize> {
    ops.iter()
        .enumerate()
        .filter(|(_, op)| is_show(&op.operator) && !shown_strings(op).is_empty())
        .map(|(i, _)| i)
        .collect()
}

/// Removes the given text objects (by their order among the page's text objects)
/// and draws `overlay_page_pdf` (a one-page PDF of the same size) on top.
pub fn remove_and_overlay(
    original: &[u8],
    page_index: u32,
    text_ordinals: &[usize],
    expected_text_objects: usize,
    overlay_page_pdf: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut doc = Document::load_mem(original).map_err(|e| err(e.to_string()))?;
    if doc.is_encrypted() {
        return Err(err("document chiffré"));
    }
    let page_id = *doc
        .get_pages()
        .get(&(page_index + 1))
        .ok_or(KbError::BadPage(page_index + 1))?;
    let raw = doc.get_page_content(page_id).map_err(|e| err(e.to_string()))?;
    let mut content = Content::decode(&raw).map_err(|e| err(e.to_string()))?;

    let shows = text_ops(&content.operations);
    if shows.len() != expected_text_objects {
        return Err(err(format!(
            "structure de page inattendue ({} opérateurs texte, {} objets)",
            shows.len(),
            expected_text_objects
        )));
    }
    let targets: HashSet<usize> = text_ordinals
        .iter()
        .map(|&o| shows.get(o).copied().ok_or_else(|| err("objet texte introuvable")))
        .collect::<Result<_>>()?;

    // Width tables for every font of the page.
    let fonts: BTreeMap<Vec<u8>, Widths> = {
        let page_fonts = doc.get_page_fonts(page_id).map_err(|e| err(e.to_string()))?;
        page_fonts
            .into_iter()
            .filter_map(|(name, dict)| font_widths(&doc, dict).ok().map(|w| (name, w)))
            .collect()
    };

    // For each target: does a later text-show on the same line depend on its width?
    // (If the next text operator moves to a new position, the width doesn't matter.)
    let needs_advance: HashSet<usize> = targets
        .iter()
        .copied()
        .filter(|&i| {
            content.operations[i + 1..]
                .iter()
                .enumerate()
                .find(|(_, op)| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"" | "Td" | "TD" | "Tm" | "T*" | "ET"))
                .is_some_and(|(k, op)| matches!(op.operator.as_str(), "Tj" | "TJ") && !targets.contains(&(i + 1 + k)))
        })
        .collect();

    let mut state = TextState { font: vec![], size: 0.0, tc: 0.0, tw: 0.0 };
    let mut stack: Vec<TextState> = vec![];
    let mut new_ops: Vec<Operation> = Vec::with_capacity(content.operations.len() + 8);

    for (i, op) in content.operations.drain(..).enumerate() {
        match op.operator.as_str() {
            "q" => stack.push(state.clone()),
            "Q" => {
                if let Some(s) = stack.pop() {
                    state = s;
                }
            }
            "Tf" => {
                if let (Some(n), Some(s)) = (op.operands.first().and_then(|o| o.as_name().ok()), op.operands.get(1).and_then(num)) {
                    state.font = n.to_vec();
                    state.size = s;
                }
            }
            "Tc" => state.tc = op.operands.first().and_then(num).unwrap_or(state.tc),
            "Tw" => state.tw = op.operands.first().and_then(num).unwrap_or(state.tw),
            "\"" => {
                state.tw = op.operands.first().and_then(num).unwrap_or(state.tw);
                state.tc = op.operands.get(1).and_then(num).unwrap_or(state.tc);
            }
            _ => {}
        }
        if !targets.contains(&i) {
            new_ops.push(op);
            continue;
        }

        // Nothing on this line depends on the width: simply drop the text.
        if !needs_advance.contains(&i) {
            match op.operator.as_str() {
                "'" => new_ops.push(Operation::new("T*", vec![])),
                "\"" => {
                    new_ops.push(Operation::new("Tw", vec![op.operands[0].clone()]));
                    new_ops.push(Operation::new("Tc", vec![op.operands[1].clone()]));
                    new_ops.push(Operation::new("T*", vec![]));
                }
                _ => {}
            }
            continue;
        }

        // Replace this show operator by a pure horizontal move of the same width.
        let w = fonts.get(&state.font).ok_or_else(|| err("largeurs de police inconnues"))?;
        if state.size.abs() < 1e-6 {
            return Err(err("taille de police nulle"));
        }
        let advance = |s: &[u8]| -> f32 {
            w.codes(s)
                .map(|c| {
                    let space = !w.two_byte && c == 32;
                    w.width(c) / 1000.0 * state.size + state.tc + if space { state.tw } else { 0.0 }
                })
                .sum::<f32>()
        };
        let mut tj: f32 = 0.0; // TJ units: thousandths of text space, negative = move right
        match op.operator.as_str() {
            "TJ" => {
                for item in op.operands.first().and_then(|o| o.as_array().ok()).into_iter().flatten() {
                    match item {
                        Object::String(s, _) => tj -= advance(s) * 1000.0 / state.size,
                        other => tj += num(other).unwrap_or(0.0),
                    }
                }
            }
            "Tj" | "'" => {
                if op.operator == "'" {
                    new_ops.push(Operation::new("T*", vec![]));
                }
                tj -= advance(op.operands[0].as_str().unwrap_or(b"")) * 1000.0 / state.size;
            }
            "\"" => {
                new_ops.push(Operation::new("Tw", vec![op.operands[0].clone()]));
                new_ops.push(Operation::new("Tc", vec![op.operands[1].clone()]));
                new_ops.push(Operation::new("T*", vec![]));
                tj -= advance(op.operands[2].as_str().unwrap_or(b"")) * 1000.0 / state.size;
            }
            _ => unreachable!(),
        }
        new_ops.push(Operation::new("TJ", vec![Object::Array(vec![Object::Real(tj)])]));
    }

    // Isolate the original drawing so its state can't affect what we add.
    let mut ops = vec![Operation::new("q", vec![])];
    ops.extend(new_ops);
    ops.push(Operation::new("Q", vec![]));

    if let Some(overlay) = overlay_page_pdf {
        // Appended as ordinary page text (not a separate drawing) so it can be edited again later.
        let overlay_ops = merge_overlay(&mut doc, page_id, overlay)?;
        ops.push(Operation::new("q", vec![]));
        ops.extend(overlay_ops);
        ops.push(Operation::new("Q", vec![]));
    }

    let encoded = Content { operations: ops }.encode().map_err(|e| err(e.to_string()))?;
    // Always write a fresh single stream: /Contents may be a stream, an array,
    // or a reference to an array, and may be shared with other pages.
    let mut stream = lopdf::Stream::new(Dictionary::new(), encoded);
    let _ = stream.compress();
    let stream_id = doc.add_object(stream);
    doc.get_dictionary_mut(page_id)
        .map_err(|e| err(e.to_string()))?
        .set("Contents", Object::Reference(stream_id));
    // Drop what is no longer used (old content, old fonts) so removed text
    // does not linger in the file.
    doc.prune_objects();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| err(e.to_string()))?;
    Ok(out)
}

/// Copies the drawing of page 1 of `overlay` into `doc`: its resources (fonts,
/// graphic states) are added to the target page under unique names, and its
/// operators are returned with those names.
fn merge_overlay(doc: &mut Document, target_page: ObjectId, overlay: &[u8]) -> Result<Vec<Operation>> {
    let mut src = Document::load_mem(overlay).map_err(|e| err(e.to_string()))?;
    src.renumber_objects_with(doc.max_id + 1);
    let src_page = *src.get_pages().get(&1).ok_or_else(|| err("calque vide"))?;
    let raw = src.get_page_content(src_page).map_err(|e| err(e.to_string()))?;
    let mut ops = Content::decode(&raw).map_err(|e| err(e.to_string()))?.operations;
    let resources = match src.get_dictionary(src_page).and_then(|p| p.get(b"Resources")) {
        Ok(o) => deref(&src, o).as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };

    // Copy every object reachable from the resources (fonts, font files…).
    let mut todo: Vec<ObjectId> = vec![];
    collect_refs(&Object::Dictionary(resources.clone()), &mut todo);
    let mut seen = HashSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Ok(obj) = src.get_object(id) {
            collect_refs(obj, &mut todo);
            doc.objects.insert(id, obj.clone());
        }
    }
    doc.max_id = doc.max_id.max(src.max_id);

    // Add each resource to the page under a unique name, and rename its uses.
    let tag = doc.max_id;
    for (category, operators) in [(&b"Font"[..], &["Tf"][..]), (b"ExtGState", &["gs"]), (b"XObject", &["Do"]), (b"ColorSpace", &["cs", "CS"])] {
        let Ok(entries) = resources.get(category).map(|o| deref(&src, o)).and_then(Object::as_dict) else { continue };
        for (name, value) in entries.iter() {
            let new_name = [b"Kb".as_slice(), tag.to_string().as_bytes(), b"_", name].concat();
            add_page_resource(doc, target_page, category, &new_name, value.clone())?;
            for op in ops.iter_mut().filter(|op| operators.contains(&op.operator.as_str())) {
                if let Some(Object::Name(n)) = op.operands.first_mut() {
                    if n == name {
                        *n = new_name.clone();
                    }
                }
            }
        }
    }
    Ok(ops)
}

/// Sets Resources/<category>/<name> on a page, creating dictionaries as needed.
/// Inherited resources are copied onto the page first so other pages are untouched.
fn add_page_resource(doc: &mut Document, page: ObjectId, category: &[u8], name: &[u8], value: Object) -> Result<()> {
    // Resolve the page's effective resources into a direct dictionary on the page.
    let (direct, inherited) = doc.get_page_resources(page).map_err(|e| err(e.to_string()))?;
    let mut res = match direct {
        Some(d) => d.clone(),
        None => inherited
            .first()
            .and_then(|id| doc.get_dictionary(*id).ok().cloned())
            .unwrap_or_default(),
    };
    let mut cat = match res.get(category) {
        Ok(o) => deref(doc, o).as_dict().cloned().unwrap_or_default(),
        Err(_) => Dictionary::new(),
    };
    cat.set(name.to_vec(), value);
    res.set(category.to_vec(), Object::Dictionary(cat));
    doc.get_dictionary_mut(page)
        .map_err(|e| err(e.to_string()))?
        .set("Resources", Object::Dictionary(res));
    Ok(())
}

fn collect_refs(o: &Object, out: &mut Vec<ObjectId>) {
    match o {
        Object::Reference(id) => out.push(*id),
        Object::Array(a) => a.iter().for_each(|x| collect_refs(x, out)),
        Object::Dictionary(d) => d.iter().for_each(|(_, v)| collect_refs(v, out)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, v)| collect_refs(v, out)),
        _ => {}
    }
}

