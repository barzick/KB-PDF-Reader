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
