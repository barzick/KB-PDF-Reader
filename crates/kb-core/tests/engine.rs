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

/// A plain Latin TrueType font present on the machine (Liberation/DejaVu on Linux,
/// Arial on macOS/Windows).
fn any_ttf() -> String {
    const PREFERRED: [&str; 4] = ["LiberationSans-Regular.ttf", "DejaVuSans.ttf", "Arial.ttf", "arial.ttf"];
    let mut found = vec![];
    for dir in ["/usr/share/fonts", "/System/Library/Fonts", "/Library/Fonts", "C:\\Windows\\Fonts"] {
        walk(std::path::Path::new(dir), &mut found);
    }
    for name in PREFERRED {
        if let Some(p) = found.iter().find(|p| p.ends_with(name)) {
            return p.clone();
        }
    }
    panic!("aucune police de test (Liberation Sans, DejaVu Sans ou Arial) trouvée");
}

fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p.display().to_string());
        }
    }
}

#[test]
fn edit_paragraph_exactly_and_again() {
    setup();
    let doc = new_text_document(&["Bonjour le monde"]).unwrap();
    let blocks = text_blocks(&doc, 0).unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].text, "Bonjour le monde");

    let font = any_ttf();
    let r = replace_block(&doc, 0, &blocks[0], "Au revoir, été 2026", vec![WriteFont::File(font.as_ref())], EditMode::Exact).unwrap();
    assert!(r.lossless, "l'édition doit garder le reste de la page intact");
    let after = text_blocks(&r.bytes, 0).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].text, "Au revoir, été 2026");
    assert!((after[0].y - blocks[0].y).abs() < 0.5, "même position");

    // The new text must be editable again.
    let r2 = replace_block(&r.bytes, 0, &after[0], "Troisième version", vec![WriteFont::File(font.as_ref())], EditMode::Exact).unwrap();
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
    let r = replace_block(&doc, 0, &blocks[0], &long, vec![WriteFont::File(font.as_ref())], EditMode::ShiftBelow).unwrap();
    assert!(r.extra_lines > 0);
    assert!(!r.lossless);
}

#[test]
fn edit_page_with_several_content_streams() {
    use pdfium_render::prelude::*;
    setup();
    // PDFium writes one content stream per added object, referenced through an array.
    let pdfium = Pdfium::default();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let font = doc.fonts_mut().load_true_type_from_file(&any_ttf(), true).unwrap();
    let mut page = doc.pages_mut().create_page_at_end(PdfPagePaperSize::a4()).unwrap();
    for (i, line) in ["Titre", "Ligne un du paragraphe qui est assez longue pour aller", "jusqu'à la marge."].iter().enumerate() {
        let y = if i == 0 { 760.0 } else { 720.0 - 14.0 * (i as f32 - 1.0) };
        page.objects_mut().create_text_object(PdfPoints::new(72.0), PdfPoints::new(y), *line, font, PdfPoints::new(11.0)).unwrap();
    }
    drop(page);
    let bytes = doc.save_to_bytes().unwrap();
    let blocks = text_blocks(&bytes, 0).unwrap();
    assert_eq!(blocks.len(), 2);

    let r = replace_block(&bytes, 0, &blocks[1], "Texte remplacé.", vec![WriteFont::File(any_ttf().as_ref())], EditMode::Exact).unwrap();
    assert!(r.lossless);
    let texts: Vec<String> = text_blocks(&r.bytes, 0).unwrap().into_iter().map(|b| b.text).collect();
    assert_eq!(texts, vec!["Titre".to_string(), "Texte remplacé.".to_string()]);
}

/// Turns a text document into a "scan" (one image per page).
fn scan_of(pdf: &[u8]) -> Vec<u8> {
    let n = info(pdf).unwrap().page_count;
    let pngs: Vec<Vec<u8>> = (0..n).map(|p| render_png(pdf, p, 2480).unwrap()).collect();
    let refs: Vec<&[u8]> = pngs.iter().map(|v| v.as_slice()).collect();
    images_to_pdf(&refs, PageFormat::A4, 0.0).unwrap()
}

#[test]
fn images_and_blank_pages() {
    setup();
    let scan = scan_of(&new_text_document(&["Une page", "Deux pages"]).unwrap());
    let i = info(&scan).unwrap();
    assert_eq!(i.page_count, 2);
    assert!((i.pages[0].width - 595.0).abs() < 1.0 && (i.pages[0].height - 842.0).abs() < 1.0);
    let blank = blank_document(3, PageFormat::Letter, true).unwrap();
    let i = info(&blank).unwrap();
    assert_eq!(i.page_count, 3);
    assert!((i.pages[0].width - 792.0).abs() < 1.0);

    let doc = new_text_document(&["Un", "Deux"]).unwrap();
    let more = insert_blank_page(&doc, 1).unwrap();
    assert_eq!(info(&more).unwrap().page_count, 3);
    assert!(text_blocks(&more, 1).unwrap().is_empty());
    assert!(text_blocks(&more, 2).unwrap()[0].text.contains("Deux"));
}

#[cfg(feature = "ocr")]
#[test]
fn ocr_makes_scans_searchable_and_editable() {
    setup();
    let tessdata = std::env::var("TESSDATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/tessdata"));
    if !tessdata.join("fra.traineddata").exists() {
        eprintln!("tessdata absent ({}) : test OCR ignoré", tessdata.display());
        return;
    }
    let scan = scan_of(&new_text_document(&["Facture numéro 2026", "Merci beaucoup"]).unwrap());
    assert_eq!(scanned_pages(&scan).unwrap(), vec![0, 1]);
    assert!(text_blocks(&scan, 0).unwrap().is_empty());

    let mut calls = 0;
    let (out, summary) = make_searchable(&scan, None, &tessdata, "fra+eng", |_, _| calls += 1).unwrap();
    assert_eq!(calls, 2);
    assert_eq!(summary.pages, vec![1, 2]);
    assert!(summary.confidence > 70.0, "{summary:?}");
    assert!(scanned_pages(&out).unwrap().is_empty());

    // The page looks exactly the same (the text layer is invisible).
    let (before, after) = (render_png(&scan, 0, 600).unwrap(), render_png(&out, 0, 600).unwrap());
    assert!(before == after, "la couche OCR ne doit pas être visible");

    let blocks = text_blocks(&out, 0).unwrap();
    assert!(blocks.iter().all(|b| b.scanned));
    let text: String = blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join(" ");
    assert!(text.contains("Facture") && text.contains("2026"), "{text}");
    // The text layer sits where the words are printed (y ≈ 760).
    let b = blocks.iter().find(|b| b.text.contains("Facture")).unwrap();
    assert!((b.y - 760.0).abs() < 4.0 && (b.x - 72.0).abs() < 4.0, "x={} y={}", b.x, b.y);
    assert!((b.size - 24.0).abs() < 4.0, "size={}", b.size);

    // Running it again changes nothing (pages already have text).
    let (_, again) = make_searchable(&out, None, &tessdata, "fra", |_, _| {}).unwrap();
    assert!(again.pages.is_empty() && again.skipped == vec![1, 2]);
}

#[test]
fn search_and_page_text() {
    setup();
    let doc = new_text_document(&["Facture numéro 12", "Rien ici", "La facture suit"]).unwrap();
    let t = page_text(&doc, 0).unwrap();
    assert!(t.text.contains("Facture numéro 12"));
    assert_eq!(t.text.chars().count(), t.boxes.len());
    let hits = search(&doc, "FACTURE", false).unwrap();
    assert_eq!(hits.iter().map(|h| h.page).collect::<Vec<_>>(), vec![0, 2]);
    let r = hits[0].rects[0];
    assert!(r[0] > 70.0 && r[0] < 75.0 && r[1] > 750.0 && r[3] < 790.0, "{r:?}");
    assert_eq!(search(&doc, "numero", false).unwrap().len(), 1, "sans accents");
    assert!(search(&doc, "FACTURE", true).unwrap().is_empty());
}

/// Fraction of non-white pixels of a page area (PDF points), rendered at 1 px/pt.
/// Share of near-black pixels in a page area.
fn dark(pdf: &[u8], rect: [f32; 4]) -> f32 {
    let png = render_png(pdf, 0, 595).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_luma8();
    let h = img.height() as f32;
    let (mut n, mut d) = (0, 0);
    for y in (h - rect[3]).max(0.0) as u32..(h - rect[1]).min(h) as u32 {
        for x in rect[0].max(0.0) as u32..(rect[2] as u32).min(img.width()) {
            n += 1;
            d += (img.get_pixel(x, y).0[0] < 90) as u32;
        }
    }
    d as f32 / n.max(1) as f32
}

fn ink(pdf: &[u8], rect: [f32; 4]) -> f32 {
    let png = render_png(pdf, 0, 595).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    let h = img.height() as f32;
    let (mut n, mut dark) = (0, 0);
    for y in (h - rect[3]).max(0.0) as u32..(h - rect[1]).min(h) as u32 {
        for x in rect[0].max(0.0) as u32..(rect[2] as u32).min(img.width()) {
            n += 1;
            let p = img.get_pixel(x, y).0;
            if p[0] < 230 || p[1] < 230 || p[2] < 230 {
                dark += 1;
            }
        }
    }
    dark as f32 / n.max(1) as f32
}

#[test]
fn annotations_show_list_and_delete() {
    setup();
    let doc = blank_document(1, PageFormat::A4, false).unwrap();
    let all = vec![
        (NewAnnot::Highlight { rects: vec![[50.0, 700.0, 200.0, 715.0]], color: [255, 230, 0] }, [60.0, 702.0, 190.0, 713.0]),
        (NewAnnot::Underline { rects: vec![[50.0, 650.0, 200.0, 665.0]], color: [0, 0, 255] }, [60.0, 650.0, 190.0, 653.0]),
        (NewAnnot::StrikeOut { rects: vec![[50.0, 600.0, 200.0, 615.0]], color: [255, 0, 0] }, [60.0, 606.0, 190.0, 609.0]),
        (NewAnnot::Note { x: 300.0, y: 720.0, text: "Vérifier le montant".into(), color: [255, 220, 80] }, [302.0, 702.0, 318.0, 718.0]),
        (NewAnnot::FreeText { rect: [300.0, 600.0, 500.0, 680.0], text: "Bonjour à tous, ceci est une zone de texte".into(), size: 14.0, color: [0, 0, 0], border: true }, [300.0, 600.0, 500.0, 680.0]),
        (NewAnnot::Square { rect: [50.0, 450.0, 150.0, 550.0], color: [255, 0, 0], width: 2.0, fill: Some([200, 230, 255]) }, [60.0, 460.0, 140.0, 540.0]),
        (NewAnnot::Circle { rect: [200.0, 450.0, 300.0, 550.0], color: [0, 128, 0], width: 3.0, fill: None }, [200.0, 450.0, 300.0, 550.0]),
        (NewAnnot::Line { from: [350.0, 450.0], to: [500.0, 550.0], color: [0, 0, 0], width: 2.0, arrow: true }, [350.0, 450.0, 500.0, 550.0]),
        (NewAnnot::Ink { strokes: vec![vec![[50.0, 300.0], [100.0, 350.0], [150.0, 300.0]]], color: [0, 0, 200], width: 3.0 }, [50.0, 295.0, 150.0, 355.0]),
        (NewAnnot::Stamp { rect: [300.0, 300.0, 500.0, 360.0], label: "Approuvé".into(), color: [200, 0, 0] }, [300.0, 300.0, 500.0, 360.0]),
    ];
    let mut pdf = doc.clone();
    for (a, area) in &all {
        let before = ink(&pdf, *area);
        pdf = add_annotation(&pdf, 0, a).unwrap();
        let after = ink(&pdf, *area);
        assert!(after > before + 0.02, "{a:?} invisible ({before} → {after})");
    }
    let list = list_annotations(&pdf, 0).unwrap();
    assert_eq!(list.len(), all.len());
    assert_eq!(list[3].subtype, "Text");
    assert_eq!(list[3].contents, "Vérifier le montant");
    let pdf = set_note_text(&pdf, 0, list[3].index, "OK").unwrap();
    assert_eq!(list_annotations(&pdf, 0).unwrap()[3].contents, "OK");
    let pdf = delete_annotation(&pdf, 0, list[0].index).unwrap();
    assert_eq!(list_annotations(&pdf, 0).unwrap().len(), all.len() - 1);
    assert!(ink(&pdf, [60.0, 702.0, 190.0, 713.0]) < 0.01, "surlignage supprimé");
}

#[test]
fn forms_fill_and_flatten() {
    setup();
    let pdf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/form.pdf")).unwrap();
    let fields = list_fields(&pdf).unwrap();
    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["nom", "ville", "remarques", "accepte", "taille", "taille", "taille", "pays"]);
    assert_eq!(fields[0].kind, FieldKind::Text);
    assert!(fields[2].multiline);
    assert_eq!(fields[3].kind, FieldKind::Checkbox);
    assert_eq!(fields[5].export, "M");
    assert_eq!(fields[7].kind, FieldKind::Combo);
    assert_eq!(fields[7].options, ["Suisse", "France", "Sénégal"]);

    let nom = fields[0].rect;
    let before = dark(&pdf, [nom[0] + 3.0, nom[1] + 3.0, nom[2] - 3.0, nom[3] - 3.0]);
    let v = |n: &str, x: &str| FieldValue { name: n.into(), value: x.into() };
    let filled = fill_fields(
        &pdf,
        &[
            v("nom", "Jean Dupont-Ndiaye"),
            v("ville", "Genève"),
            v("remarques", "Première ligne\nDeuxième ligne assez longue pour être coupée automatiquement à la bonne largeur"),
            v("accepte", "true"),
            v("taille", "M"),
            v("pays", "Sénégal"),
        ],
    )
    .unwrap();
    let after = dark(&filled, [nom[0] + 3.0, nom[1] + 3.0, nom[2] - 3.0, nom[3] - 3.0]);
    assert!(after > before + 0.03, "nom invisible ({before} → {after})");

    let f = list_fields(&filled).unwrap();
    assert_eq!(f[0].value, "Jean Dupont-Ndiaye");
    assert_eq!(f[1].value, "Genève");
    assert!(f[3].checked);
    assert!(!f[4].checked && f[5].checked && !f[6].checked);
    assert_eq!(f[7].value, "Sénégal");
    let chk = f[3].rect;
    assert!(dark(&filled, chk) > dark(&pdf, chk) + 0.03, "case non cochée à l'écran");

    // Flattening keeps the look but removes the fields.
    let flat = flatten(&filled).unwrap();
    assert!(list_fields(&flat).unwrap().is_empty());
    let a = dark(&flat, [nom[0] + 3.0, nom[1] + 3.0, nom[2] - 3.0, nom[3] - 3.0]);
    assert!((a - after).abs() < 0.03, "rendu changé à l'aplatissement ({after} → {a})");
}

#[test]
fn image_stamp_with_transparency() {
    setup();
    let doc = blank_document(1, PageFormat::A4, false).unwrap();
    // A blue stroke on a transparent background, like a drawn signature.
    let mut img = image::RgbaImage::new(200, 80);
    for x in 20..180 {
        for y in 35..45 {
            img.put_pixel(x, y, image::Rgba([0, 0, 200, 255]));
        }
    }
    let mut png = vec![];
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let rect = [100.0, 100.0, 300.0, 180.0];
    let pdf = add_annotation(&doc, 0, &NewAnnot::Image { rect, data: png }).unwrap();
    // Stroke drawn, transparent corners left white.
    assert!(ink(&pdf, [130.0, 137.0, 270.0, 143.0]) > 0.5);
    assert!(ink(&pdf, [102.0, 160.0, 140.0, 178.0]) < 0.01);
    assert_eq!(list_annotations(&pdf, 0).unwrap()[0].subtype, "Stamp");
}

#[test]
fn text_watermark_and_page_numbers() {
    setup();
    let doc = new_text_document(&["Page un", "Page deux", "Page trois"]).unwrap();
    let style = |size: f32| TextStyle { family: StandardFamily::Sans, bold: false, italic: false, size, color: [0, 0, 0], opacity: 1.0 };

    let with_text = add_text(&doc, 0, 300.0, 500.0, "Ajouté ici\nDeuxième ligne", style(14.0)).unwrap();
    let t = page_text(&with_text, 0).unwrap().text;
    assert!(t.contains("Ajouté ici") && t.contains("Deuxième ligne"), "{t}");
    assert!(page_text(&with_text, 0).unwrap().text.contains("Page un"), "texte d'origine conservé");

    let wm = Watermark {
        text: "CONFIDENTIEL".into(),
        style: TextStyle { size: 60.0, color: [200, 0, 0], opacity: 0.3, ..style(60.0) },
        angle: 45.0,
        pages: None,
    };
    let marked = add_watermark(&doc, &wm).unwrap();
    for p in 0..3 {
        assert!(page_text(&marked, p).unwrap().text.contains("CONFIDENTIEL"));
    }
    // Light (semi-transparent) red in the middle of the page.
    let png = render_png(&marked, 0, 595).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    let reddish = img.pixels().filter(|p| p[0] > 200 && p[1] > 120 && p[1] < 220 && p[2] < 220 && p[1] == p[2]).count();
    assert!(reddish > 500, "filigrane invisible ({reddish})");

    let hf = HeaderFooter {
        header: ["Rapport".into(), String::new(), "{date}".into()],
        footer: [String::new(), "Page {page} / {pages}".into(), String::new()],
        style: style(9.0),
        margin: 30.0,
        start: 1,
        pages: None,
        date: "12.11.2026".into(),
        file: String::new(),
    };
    let numbered = add_header_footer(&doc, &hf).unwrap();
    let t = page_text(&numbered, 1).unwrap().text;
    assert!(t.contains("Page 2 / 3") && t.contains("Rapport") && t.contains("12.11.2026"), "{t}");
    // The footer sits at the bottom of the page.
    let hits = search(&numbered, "Page 3 / 3", false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 2);
    assert!(hits[0].rects[0][1] < 45.0, "{:?}", hits[0].rects);
}
