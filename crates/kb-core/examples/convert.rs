//! Converts a PDF to .txt, .docx, .xlsx and page images (manual check).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (src, out) = (&a[1], &a[2]);
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let bytes = std::fs::read(src).unwrap();
    let n = kb_core::info(&bytes).unwrap().page_count;
    let pages: Vec<u32> = (0..n).collect();
    std::fs::write(format!("{out}.txt"), kb_core::to_text(&bytes, &pages).unwrap()).unwrap();
    std::fs::write(format!("{out}.docx"), kb_core::to_docx(&bytes, &pages, |_, _| {}).unwrap()).unwrap();
    std::fs::write(format!("{out}.xlsx"), kb_core::to_xlsx(&bytes, &pages, |_, _| {}).unwrap()).unwrap();
    kb_core::to_images(&bytes, &pages, 72, kb_core::ImageKind::Jpeg, |_, p, d| {
        std::fs::write(format!("{out}-{}.jpg", p + 1), d).map_err(|e| kb_core::KbError::Image(e.to_string()))
    })
    .unwrap();
}
