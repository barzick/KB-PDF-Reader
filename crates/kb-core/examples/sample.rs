//! Writes sample PDFs for manual testing: cargo run --example sample -- <out_dir>
fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let a = kb_core::new_text_document(&["Facture n°1 - page 1", "Facture n°1 - page 2", "Facture n°1 - page 3"]).unwrap();
    let b = kb_core::new_text_document(&["Contrat - page 1", "Contrat - page 2"]).unwrap();
    std::fs::write(format!("{out}/facture.pdf"), a).unwrap();
    std::fs::write(format!("{out}/contrat.pdf"), b).unwrap();
}
