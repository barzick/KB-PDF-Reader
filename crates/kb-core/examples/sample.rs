//! Writes sample PDFs for manual testing: cargo run --example sample -- <out_dir> [ttf...]
use pdfium_render::prelude::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| ".".into());
    let ttfs: Vec<String> = args.collect();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let a = kb_core::new_text_document(&["Facture n°1 - page 1", "Facture n°1 - page 2", "Facture n°1 - page 3"]).unwrap();
    let b = kb_core::new_text_document(&["Contrat - page 1", "Contrat - page 2"]).unwrap();
    std::fs::write(format!("{out}/facture.pdf"), a).unwrap();
    std::fs::write(format!("{out}/contrat.pdf"), b).unwrap();

    // A document mixing standard and embedded fonts, to try the Fonts panel.
    let pdfium = Pdfium::default();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let mut fonts = vec![
        doc.fonts_mut().times_roman(),
        doc.fonts_mut().courier_bold(),
        doc.fonts_mut().helvetica_oblique(),
    ];
    for t in &ttfs {
        fonts.push(doc.fonts_mut().load_true_type_from_file(t, true).unwrap());
    }
    let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
    for (i, f) in fonts.into_iter().enumerate() {
        page.objects_mut()
            .create_text_object(PdfPoints::new(72.0), PdfPoints::new(760.0 - 40.0 * i as f32), "Polices mélangées 123", f, PdfPoints::new(20.0))
            .unwrap();
    }
    std::fs::write(format!("{out}/polices.pdf"), doc.save_to_bytes().unwrap()).unwrap();

    // A letter with multi-line paragraphs, one text object per line (like Word exports).
    if let Some(ttf) = ttfs.first() {
        let mut doc = pdfium.create_new_pdf().unwrap();
        let font = doc.fonts_mut().load_true_type_from_file(ttf, true).unwrap();
        let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
        let paras: [&[&str]; 4] = [
            &["Société Exemple SA", "Rue du Lac 12", "1000 Lausanne"],
            &["Objet : confirmation de commande n° 2026-114"],
            &[
                "Madame, Monsieur, nous vous confirmons la bonne réception de votre commande",
                "du 3 novembre. La livraison est prévue pour la fin du mois, à l'adresse",
                "indiquée sur le bon de commande.",
            ],
            &[
                "Le montant total de 1 250,00 CHF sera facturé à la livraison. Pour toute",
                "question, notre service client reste à votre disposition.",
            ],
        ];
        let mut y = 760.0;
        for p in paras {
            for line in p {
                page.objects_mut()
                    .create_text_object(PdfPoints::new(72.0), PdfPoints::new(y), *line, font, PdfPoints::new(11.0))
                    .unwrap();
                y -= 14.0;
            }
            y -= 14.0;
        }
        std::fs::write(format!("{out}/lettre.pdf"), doc.save_to_bytes().unwrap()).unwrap();
    }
}
