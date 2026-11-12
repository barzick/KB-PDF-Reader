//! Bookmarks (the document outline).

use lopdf::{dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::annot::text;
use crate::{pdfium, KbError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub title: String,
    /// 0-based page, None when the bookmark points nowhere in this file.
    pub page: Option<u32>,
    #[serde(default)]
    pub children: Vec<Bookmark>,
}

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

fn target(b: &PdfBookmark) -> Option<u32> {
    if let Some(d) = b.destination() {
        return d.page_index().ok().map(|p| p as u32);
    }
    let a = b.action()?;
    let local = a.as_local_destination_action()?;
    local.destination().ok()?.page_index().ok().map(|p| p as u32)
}

fn read(first: Option<PdfBookmark>, depth: usize) -> Vec<Bookmark> {
    let mut out = vec![];
    let mut cur = first;
    // Guard against looping outlines in broken files.
    let mut guard = 0;
    while let Some(b) = cur {
        guard += 1;
        if guard > 10_000 {
            break;
        }
        let children = if depth < 32 { read(b.first_child(), depth + 1) } else { vec![] };
        out.push(Bookmark { title: b.title().unwrap_or_default(), page: target(&b), children });
        cur = b.next_sibling();
    }
    out
}

/// The document's bookmarks as a tree.
pub fn bookmarks(bytes: &[u8]) -> Result<Vec<Bookmark>> {
    let doc = pdfium()?.load_pdf_from_byte_slice(bytes, None)?;
    let root = doc.bookmarks().root();
    Ok(read(root, 0))
}

/// Adds the items of `list` under `parent`; returns (first, last, visible count).
fn write(doc: &mut Document, pages: &[ObjectId], list: &[Bookmark], parent: ObjectId) -> (Option<ObjectId>, Option<ObjectId>, i64) {
    let ids: Vec<ObjectId> = list.iter().map(|_| doc.new_object_id()).collect();
    let mut count = 0;
    for (i, b) in list.iter().enumerate() {
        let mut d = dictionary! {
            "Title" => text(&b.title),
            "Parent" => parent,
        };
        if let Some(&page) = b.page.and_then(|p| pages.get(p as usize)) {
            d.set("Dest", Object::Array(vec![page.into(), "XYZ".into(), Object::Null, Object::Null, Object::Null]));
        }
        if i > 0 {
            d.set("Prev", ids[i - 1]);
        }
        if i + 1 < ids.len() {
            d.set("Next", ids[i + 1]);
        }
        let (first, last, n) = write(doc, pages, &b.children, ids[i]);
        if let (Some(f), Some(l)) = (first, last) {
            d.set("First", f);
            d.set("Last", l);
            // Negative: closed (children hidden until opened).
            d.set("Count", -n);
        }
        doc.objects.insert(ids[i], Object::Dictionary(d));
        count += 1;
    }
    (ids.first().copied(), ids.last().copied(), count)
}

/// Replaces the whole outline.
pub fn set_bookmarks(bytes: &[u8], list: &[Bookmark]) -> Result<Vec<u8>> {
    let mut doc = Document::load_mem(bytes).map_err(err)?;
    if doc.is_encrypted() {
        return Err(err("document protégé par mot de passe"));
    }
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let root = doc.trailer.get(b"Root").and_then(|o| o.as_reference()).map_err(err)?;
    if list.is_empty() {
        let cat = doc.get_object_mut(root).and_then(|o| o.as_dict_mut()).map_err(err)?;
        cat.remove(b"Outlines");
        if cat.get(b"PageMode").ok().and_then(|o| o.as_name().ok()) == Some(b"UseOutlines") {
            cat.remove(b"PageMode");
        }
    } else {
        let outlines = doc.new_object_id();
        let (first, last, n) = write(&mut doc, &pages, list, outlines);
        let mut d = dictionary! { "Type" => "Outlines", "Count" => n };
        if let (Some(f), Some(l)) = (first, last) {
            d.set("First", f);
            d.set("Last", l);
        }
        doc.objects.insert(outlines, Object::Dictionary(d));
        let cat = doc.get_object_mut(root).and_then(|o| o.as_dict_mut()).map_err(err)?;
        cat.set("Outlines", outlines);
    }
    doc.prune_objects();
    let mut out = vec![];
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}
