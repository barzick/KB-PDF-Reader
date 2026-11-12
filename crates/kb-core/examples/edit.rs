//! Replaces paragraph N: cargo run --example edit -- in.pdf N "new text" out.pdf out.png [font.ttf...]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let bytes = std::fs::read(&a[0]).unwrap();
    let blocks = kb_core::text_blocks(&bytes, 0).unwrap();
    let b = &blocks[a[1].parse::<usize>().unwrap()];
    // One font per style of the paragraph (extra styles reuse the last one).
    let mut fonts: Vec<kb_core::WriteFont> = a[5..].iter().map(|p| kb_core::WriteFont::File(std::path::Path::new(p))).collect();
    if fonts.is_empty() {
        fonts.push(kb_core::WriteFont::Standard { bold: false, italic: false, family: kb_core::StandardFamily::Sans });
    }
    let mode = if std::env::var("SHIFT").is_ok() { kb_core::EditMode::ShiftBelow } else { kb_core::EditMode::Exact };
    let r = kb_core::replace_block(&bytes, 0, b, &a[2], fonts, mode).unwrap();
    println!("sans perte: {}  lignes en plus: {}", r.lossless, r.extra_lines);
    let out = r.bytes;
    std::fs::write(&a[3], &out).unwrap();
    std::fs::write(&a[4], kb_core::render_png(&out, 0, 1200).unwrap()).unwrap();
    println!("{} paragraphes après édition", kb_core::text_blocks(&out, 0).unwrap().len());
}
