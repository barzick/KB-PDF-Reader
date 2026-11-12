//! Comparing the text of two documents, word by word.

use serde::Serialize;
use similar::{capture_diff_slices, Algorithm, DiffOp};

use crate::{open, Result};

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Only in the new document.
    Inserted,
    /// Only in the old document.
    Deleted,
    /// Different text at the same place.
    Replaced,
}

/// A highlighted area: page (0-based) and rectangle `[left, bottom, right, top]` in PDF points.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Mark {
    pub page: u32,
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub kind: ChangeKind,
    pub old_text: String,
    pub new_text: String,
    pub old_marks: Vec<Mark>,
    pub new_marks: Vec<Mark>,
    /// Where the change is in each document (the neighbouring text for the
    /// side where it is absent).
    pub old_page: u32,
    pub new_page: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub changes: Vec<Change>,
    pub old_words: usize,
    pub new_words: usize,
}

struct Word {
    text: String,
    page: u32,
    rect: [f32; 4],
}

fn words(bytes: &[u8]) -> Result<Vec<Word>> {
    let doc = open(bytes)?;
    let mut out = vec![];
    for (p, page) in doc.pages().iter().enumerate() {
        let t = crate::text::read(&page);
        let mut cur: Option<Word> = None;
        for (c, b) in t.text.chars().zip(t.boxes.iter()) {
            if c.is_whitespace() {
                out.extend(cur.take());
                continue;
            }
            let w = cur.get_or_insert_with(|| Word { text: String::new(), page: p as u32, rect: [f32::MAX, f32::MAX, f32::MIN, f32::MIN] });
            // Typographic variants count as the same character.
            w.text.push(match c {
                '’' | '‘' => '\'',
                '“' | '”' => '"',
                '\u{a0}' | '\u{202f}' => ' ',
                c => c,
            });
            if b[2] > b[0] {
                w.rect = [w.rect[0].min(b[0]), w.rect[1].min(b[1]), w.rect[2].max(b[2]), w.rect[3].max(b[3])];
            }
        }
        out.extend(cur);
    }
    for w in &mut out {
        if w.rect[0] > w.rect[2] {
            w.rect = [0.0; 4];
        }
    }
    Ok(out)
}

/// One rectangle per line for a run of words.
fn marks(words: &[Word]) -> Vec<Mark> {
    let mut out: Vec<Mark> = vec![];
    for w in words.iter().filter(|w| w.rect[2] > w.rect[0]) {
        match out.last_mut() {
            Some(m)
                if m.page == w.page
                    && (w.rect[1] + w.rect[3]) / 2.0 > m.rect[1]
                    && (w.rect[1] + w.rect[3]) / 2.0 < m.rect[3]
                    && w.rect[0] >= m.rect[0] - 1.0 =>
            {
                m.rect = [m.rect[0].min(w.rect[0]), m.rect[1].min(w.rect[1]), m.rect[2].max(w.rect[2]), m.rect[3].max(w.rect[3])];
            }
            _ => out.push(Mark { page: w.page, rect: w.rect }),
        }
    }
    out
}

fn join(words: &[Word]) -> String {
    words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
}

/// Page of the word at `i`, or of the closest word before it.
fn page_near(words: &[Word], i: usize) -> u32 {
    words.get(i).or_else(|| words.get(i.saturating_sub(1))).or(words.last()).map(|w| w.page).unwrap_or(0)
}

/// The text differences between `old` and `new`, with where they are on the pages.
pub fn compare(old: &[u8], new: &[u8]) -> Result<Comparison> {
    let a = words(old)?;
    let b = words(new)?;
    let ta: Vec<&str> = a.iter().map(|w| w.text.as_str()).collect();
    let tb: Vec<&str> = b.iter().map(|w| w.text.as_str()).collect();
    let mut changes = vec![];
    for op in capture_diff_slices(Algorithm::Patience, &ta, &tb) {
        let (kind, oi, ol, ni, nl) = match op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete { old_index, old_len, new_index } => (ChangeKind::Deleted, old_index, old_len, new_index, 0),
            DiffOp::Insert { old_index, new_index, new_len } => (ChangeKind::Inserted, old_index, 0, new_index, new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (ChangeKind::Replaced, old_index, old_len, new_index, new_len),
        };
        let (ow, nw) = (&a[oi..oi + ol], &b[ni..ni + nl]);
        changes.push(Change {
            kind,
            old_text: join(ow),
            new_text: join(nw),
            old_marks: marks(ow),
            new_marks: marks(nw),
            old_page: page_near(&a, oi),
            new_page: page_near(&b, ni),
        });
    }
    Ok(Comparison { changes, old_words: a.len(), new_words: b.len() })
}
