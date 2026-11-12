//! Lists the paragraphs detected on a page: cargo run --example blocks -- file.pdf [page]
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("chemin du PDF");
    let page: u32 = a.next().map(|p| p.parse::<u32>().unwrap() - 1).unwrap_or(0);
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let bytes = std::fs::read(path).unwrap();
    for (i, b) in kb_core::text_blocks(&bytes, page).unwrap().iter().enumerate() {
        let preview: String = b.text.chars().take(60).collect();
        println!("{i:>3} l={} obj={:<3} x={:>5.1} y={:>5.1} w={:>5.1} t={:>4.1} lh={:>4.1} {:<22} « {preview} »",
            b.lines, b.objects.len(), b.x, b.y, b.right - b.x, b.size, b.line_height, b.font);
    }
}
