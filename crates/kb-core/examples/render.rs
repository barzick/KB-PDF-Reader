fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let b = std::fs::read(&a[0]).unwrap();
    std::fs::write(&a[1], kb_core::render_png(&b, 0, 1200).unwrap()).unwrap();
}
