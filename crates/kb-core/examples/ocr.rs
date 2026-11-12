//! OCR of a PDF: cargo run --example ocr -- in.pdf out.pdf [fra+eng]
//! (SCAN=1 first turns every page into an image, to simulate a scanned document;
//! SCAN_ONLY=1 just writes that scan.)
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let mut bytes = std::fs::read(&a[0]).unwrap();
    if std::env::var("SCAN").is_ok() {
        let n = kb_core::info(&bytes).unwrap().page_count;
        let pngs: Vec<Vec<u8>> = (0..n).map(|p| kb_core::render_png(&bytes, p, 2000).unwrap()).collect();
        let refs: Vec<&[u8]> = pngs.iter().map(|v| v.as_slice()).collect();
        bytes = kb_core::images_to_pdf(&refs, kb_core::PageFormat::A4, 0.0).unwrap();
        if std::env::var("SCAN_ONLY").is_ok() {
            std::fs::write(&a[1], &bytes).unwrap();
            return;
        }
    }
    let tessdata = std::env::var("TESSDATA_DIR").unwrap_or_else(|_| "src-tauri/tessdata".into());
    let langs = a.get(2).cloned().unwrap_or_else(|| "fra+eng".into());
    let t = std::time::Instant::now();
    let (out, s) = kb_core::make_searchable(&bytes, None, std::path::Path::new(&tessdata), &langs, |d, n| eprintln!("page {d}/{n}")).unwrap();
    println!("{s:?} en {:.1} s", t.elapsed().as_secs_f32());
    for b in kb_core::text_blocks(&out, 0).unwrap() {
        println!("[{:.0},{:.0} {:.1}pt] {}", b.x, b.y, b.size, b.text.replace('\n', "⏎"));
    }
    std::fs::write(&a[1], out).unwrap();
}
