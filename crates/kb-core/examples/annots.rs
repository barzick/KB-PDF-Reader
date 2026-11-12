//! Writes a page with one of each annotation: cargo run --example annots -- out.pdf out.png
use kb_core::*;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let mut pdf = new_text_document(&["Le montant total de 1 250,00 CHF sera facturé."]).unwrap();
    let hits = search(&pdf, "1 250,00 CHF", false).unwrap();
    let list = vec![
        NewAnnot::Highlight { rects: hits[0].rects.clone(), color: [255, 230, 0] },
        NewAnnot::Underline { rects: search(&pdf, "montant", false).unwrap()[0].rects.clone(), color: [0, 90, 255] },
        NewAnnot::StrikeOut { rects: search(&pdf, "facturé", false).unwrap()[0].rects.clone(), color: [220, 0, 0] },
        NewAnnot::Note { x: 520.0, y: 790.0, text: "Vérifier le montant".into(), color: [255, 210, 60] },
        NewAnnot::FreeText { rect: [72.0, 620.0, 300.0, 700.0], text: "Zone de texte : à payer avant le 30 novembre, merci.".into(), size: 13.0, color: [20, 20, 120], border: true },
        NewAnnot::Square { rect: [72.0, 450.0, 172.0, 550.0], color: [220, 0, 0], width: 2.0, fill: Some([255, 235, 235]) },
        NewAnnot::Circle { rect: [200.0, 450.0, 300.0, 550.0], color: [0, 140, 60], width: 3.0, fill: None },
        NewAnnot::Line { from: [330.0, 460.0], to: [480.0, 540.0], color: [0, 0, 0], width: 2.0, arrow: true },
        NewAnnot::Ink { strokes: vec![(0..40).map(|i| { let t = i as f32 / 39.0; [72.0 + 200.0 * t, 330.0 + 30.0 * (t * 12.0).sin()] }).collect()], color: [0, 0, 200], width: 2.5 },
        NewAnnot::Stamp { rect: [330.0, 310.0, 520.0, 370.0], label: "Approuvé".into(), color: [200, 0, 0] },
    ];
    for x in &list { pdf = add_annotation(&pdf, 0, x).unwrap(); }
    std::fs::write(&a[0], &pdf).unwrap();
    std::fs::write(&a[1], render_png(&pdf, 0, 1000).unwrap()).unwrap();
}
