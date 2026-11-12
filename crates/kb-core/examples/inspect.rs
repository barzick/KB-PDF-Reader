//! Prints how a PDF is built (pages, fonts, text and image objects).
//! cargo run --example inspect -- file.pdf
use pdfium_render::prelude::*;

fn main() {
    let path = std::env::args().nth(1).expect("chemin du PDF");
    let bytes = std::fs::read(&path).unwrap();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let info = kb_core::info(&bytes).unwrap();
    println!("pages: {}  taille p1: {:.0}x{:.0} pt", info.page_count, info.pages[0].width, info.pages[0].height);
    for f in kb_core::fonts(&bytes).unwrap() {
        println!("police: {:<40} famille={:<20} intégrée={} poids={} pages={:?}", f.raw_name, f.family, f.embedded, f.weight, f.pages);
    }
    let pdfium = Pdfium::default();
    let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
    for (pi, page) in doc.pages().iter().enumerate() {
        let (mut t, mut im, mut pa, mut fo, mut chars) = (0, 0, 0, 0, 0usize);
        let mut sizes = std::collections::BTreeMap::new();
        for o in page.objects().iter() {
            match o.object_type() {
                PdfPageObjectType::Text => {
                    t += 1;
                    let to = o.as_text_object().unwrap();
                    chars += to.text().chars().count();
                    *sizes.entry(to.scaled_font_size().value.round() as i32).or_insert(0) += 1;
                }
                PdfPageObjectType::Image => im += 1,
                PdfPageObjectType::Path => pa += 1,
                PdfPageObjectType::XObjectForm => fo += 1,
                _ => {}
            }
        }
        println!("page {}: objets texte={t} (caractères={chars}, moyenne {:.1}/objet) images={im} formes={pa} groupes={fo} tailles={sizes:?}", pi + 1, chars as f32 / t.max(1) as f32);
        println!("  texte extractible: {} caractères", page.text().unwrap().all().chars().count());
    }
    println!("formulaire: {:?}", doc.form().map(|f| f.form_type()));
    println!("producteur: {:?}", doc.metadata().get(PdfDocumentMetadataTagType::Producer).map(|t| t.value().to_string()));
    println!("créateur: {:?}", doc.metadata().get(PdfDocumentMetadataTagType::Creator).map(|t| t.value().to_string()));
}
