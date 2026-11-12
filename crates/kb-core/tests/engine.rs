//! Integration tests against the real PDFium library.
//! Set PDFIUM_DIR to the folder containing the library.

use kb_core::*;
use std::path::PathBuf;

fn setup() {
    let dir = std::env::var("PDFIUM_DIR").map(PathBuf::from).ok();
    init(dir.as_deref()).expect("PDFium introuvable : définir PDFIUM_DIR");
}

#[test]
fn info_render_merge_assemble_fonts() {
    setup();
    let a = new_text_document(&["Bonjour A1", "Bonjour A2"]).unwrap();
    let b = new_text_document(&["Bonjour B1"]).unwrap();

    let i = info(&a).unwrap();
    assert_eq!(i.page_count, 2);
    assert!((i.pages[0].width - 595.0).abs() < 1.0);

    let png = render_png(&a, 0, 400).unwrap();
    assert_eq!(&png[..4], b"\x89PNG");
    assert!(render_png(&a, 5, 400).is_err());

    let merged = merge(&[&a, &b]).unwrap();
    assert_eq!(info(&merged).unwrap().page_count, 3);

    // B1, A2 rotated 90°, A1
    let built = assemble(
        &[&a, &b],
        &[
            PageRef { source: 1, page: 0, rotate: 0 },
            PageRef { source: 0, page: 1, rotate: 90 },
            PageRef { source: 0, page: 0, rotate: 0 },
        ],
    )
    .unwrap();
    let bi = info(&built).unwrap();
    assert_eq!(bi.page_count, 3);
    assert!(bi.pages[1].width > bi.pages[1].height, "page 2 doit être en paysage");

    let f = fonts(&a).unwrap();
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].family, "Helvetica");
    assert_eq!(f[0].pages, vec![1, 2]);
}

fn any_ttf() -> String {
    for dir in ["/usr/share/fonts", "/System/Library/Fonts", "C:\\Windows\\Fonts"] {
        if let Some(p) = walk(std::path::Path::new(dir)) {
            return p;
        }
    }
    panic!("aucune police TrueType trouvée pour le test");
}

fn walk(dir: &std::path::Path) -> Option<String> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = walk(&p) {
                return Some(f);
            }
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ttf")) {
            return Some(p.display().to_string());
        }
    }
    None
}

#[test]
fn edit_paragraph_exactly_and_again() {
    setup();
    let doc = new_text_document(&["Bonjour le monde"]).unwrap();
    let blocks = text_blocks(&doc, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, "Bonjour le monde");

    let font = any_ttf();
    let r = replace_block(&doc, 0, &blocks[0], "Au revoir, été 2026", WriteFont::File(font.as_ref()), EditMode::Exact).unwrap();
    assert!(r.lossless, "l'édition doit garder le reste de la page intact");
    let after = text_blocks(&r.bytes, 0).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].text, "Au revoir, été 2026");
    assert!((after[0].y - blocks[0].y).abs() < 0.5, "même position");

    // The new text must be editable again.
    let r2 = replace_block(&r.bytes, 0, &after[0], "Troisième version", WriteFont::File(font.as_ref()), EditMode::Exact).unwrap();
    assert!(r2.lossless);
    let again = text_blocks(&r2.bytes, 0).unwrap();
    assert_eq!(again.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["Troisième version"]);
}

#[test]
fn shift_below_moves_following_text() {
    setup();
    // Two paragraphs on one page.
    let doc = new_text_document(&["Premier"]).unwrap();
    let blocks = text_blocks(&doc, 0).unwrap();
    let long = "mot ".repeat(200);
    let font = any_ttf();
    let r = replace_block(&doc, 0, &blocks[0], &long, WriteFont::File(font.as_ref()), EditMode::ShiftBelow).unwrap();
    assert!(r.extra_lines > 0);
    assert!(!r.lossless);
}
