//! cargo run --example decorate -- out.pdf : rotated page + watermark + footer.
use kb_core::*;
fn main() {
    init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let a = new_text_document(&["Rotation test"]).unwrap();
    let r = assemble(&[&a], &[PageRef { source: 0, page: 0, rotate: 90 }]).unwrap();
    let st = |s: f32, c: [u8; 3], o: f32| TextStyle { family: StandardFamily::Serif, bold: true, italic: false, size: s, color: c, opacity: o };
    let r = add_watermark(&r, &Watermark { text: "BROUILLON\nne pas diffuser".into(), style: st(48.0, [0, 0, 200], 0.25), angle: 30.0, pages: None }).unwrap();
    let hf = HeaderFooter { header: ["En-tête gauche".into(), String::new(), "droite".into()], footer: [String::new(), "Page {page} / {pages}".into(), String::new()], style: st(10.0, [0, 0, 0], 1.0), margin: 30.0, start: 1, pages: None, date: String::new(), file: String::new() };
    let r = add_header_footer(&r, &hf).unwrap();
    let r = add_text(&r, 0, 100.0, 400.0, "Texte ajouté", st(16.0, [0, 120, 0], 1.0)).unwrap();
    let out = std::env::args().nth(1).unwrap();
    std::fs::write(&out, &r).unwrap();
    std::fs::write(format!("{out}.png"), render_png(&r, 0, 800).unwrap()).unwrap();
}
