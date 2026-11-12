//! Page text with character positions: text selection, copy and search.

use pdfium_render::prelude::*;
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::{KbError, Result};

/// The text of a page, in reading order, with the box of every character
/// (`[left, bottom, right, top]` in PDF points; zero for spaces and line breaks
/// PDFium inferred between words or lines).
#[derive(Debug, Clone, Serialize)]
pub struct PageText {
    pub text: String,
    pub boxes: Vec<[f32; 4]>,
}

/// One search result: its page (0-based), one rectangle per line it spans,
/// and a bit of surrounding text.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub page: u32,
    pub rects: Vec<[f32; 4]>,
    pub context: String,
}

pub fn page_text(bytes: &[u8], page_index: u32) -> Result<PageText> {
    let doc = crate::open(bytes)?;
    let page = doc.pages().get(page_index as PdfPageIndex).map_err(|_| KbError::BadPage(page_index + 1))?;
    Ok(read(&page))
}

pub(crate) fn read(page: &PdfPage) -> PageText {
    let mut out = PageText { text: String::new(), boxes: vec![] };
    let Ok(text) = page.text() else { return out };
    for ch in text.chars().iter() {
        let Some(c) = ch.unicode_char() else { continue };
        // Invisible OCR text is selectable too (that is its purpose).
        let generated = ch.is_generated().unwrap_or(false);
        let b = if generated || c.is_whitespace() {
            [0.0; 4]
        } else {
            ch.loose_bounds()
                .map(|r| [r.left().value, r.bottom().value, r.right().value, r.top().value])
                .unwrap_or([0.0; 4])
        };
        // PDFium reports line ends as "\r\n": keep a single "\n".
        if c == '\r' {
            continue;
        }
        out.text.push(c);
        out.boxes.push(b);
    }
    out
}

/// Case-, accent- and spacing-insensitive form of one character
/// (always exactly one char, so positions stay aligned).
fn fold(c: char, match_case: bool) -> char {
    let c = if c.is_whitespace() || c == '\u{a0}' || c == '\u{202f}' { ' ' } else { c };
    let c = match c {
        '’' | '‘' | 'ʼ' => '\'',
        '“' | '”' | '«' | '»' => '"',
        '–' | '—' | '‐' => '-',
        _ => c,
    };
    let base = c.nfd().next().unwrap_or(c);
    if match_case {
        base
    } else {
        base.to_lowercase().next().unwrap_or(base)
    }
}

/// Merges character boxes into one rectangle per line.
pub(crate) fn line_rects(boxes: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut rects: Vec<[f32; 4]> = vec![];
    for b in boxes.iter().filter(|b| b[2] > b[0]) {
        match rects.last_mut() {
            Some(r) if (b[1] - r[1]).abs() < 0.5 * (r[3] - r[1]).max(1.0) && b[0] >= r[0] - 2.0 => {
                r[0] = r[0].min(b[0]);
                r[1] = r[1].min(b[1]);
                r[2] = r[2].max(b[2]);
                r[3] = r[3].max(b[3]);
            }
            _ => rects.push(*b),
        }
    }
    rects
}

/// Finds every occurrence of `query` in the document.
pub fn search(bytes: &[u8], query: &str, match_case: bool) -> Result<Vec<SearchHit>> {
    let needle: Vec<char> = query.trim().chars().map(|c| fold(c, match_case)).collect();
    if needle.is_empty() {
        return Ok(vec![]);
    }
    let doc = crate::open(bytes)?;
    let mut hits = vec![];
    for (p, page) in doc.pages().iter().enumerate() {
        let pt = read(&page);
        let chars: Vec<char> = pt.text.chars().collect();
        // Collapse runs of spaces so "1  250" matches "1 250".
        let mut hay: Vec<char> = vec![];
        let mut map: Vec<usize> = vec![];
        for (i, &c) in chars.iter().enumerate() {
            let f = fold(c, match_case);
            if f == ' ' && hay.last() == Some(&' ') {
                continue;
            }
            hay.push(f);
            map.push(i);
        }
        let mut i = 0;
        while i + needle.len() <= hay.len() {
            if hay[i..i + needle.len()] == needle[..] {
                let (a, b) = (map[i], map[i + needle.len() - 1]);
                let rects = line_rects(&pt.boxes[a..=b]);
                let from = a.saturating_sub(30);
                let to = (b + 31).min(chars.len());
                let context: String = chars[from..to].iter().map(|&c| if c == '\n' { ' ' } else { c }).collect();
                hits.push(SearchHit { page: p as u32, rects, context: context.trim().to_string() });
                i += needle.len();
            } else {
                i += 1;
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding() {
        let f = |s: &str| s.chars().map(|c| fold(c, false)).collect::<String>();
        assert_eq!(f("Facturé À l’ÉTÉ"), "facture a l'ete");
        assert_eq!(f("1\u{a0}250"), "1 250");
    }

    #[test]
    fn merges_lines() {
        let r = line_rects(&[[0.0, 0.0, 5.0, 10.0], [5.0, 0.0, 10.0, 10.0], [0.0; 4], [0.0, -20.0, 5.0, -10.0]]);
        assert_eq!(r, vec![[0.0, 0.0, 10.0, 10.0], [0.0, -20.0, 5.0, -10.0]]);
    }
}
