//! Prints a paragraph's text and style map: cargo run --example blocktext -- file.pdf N
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    kb_core::init(std::env::var("PDFIUM_DIR").ok().as_deref().map(std::path::Path::new)).unwrap();
    let b = &kb_core::text_blocks(&std::fs::read(&a[0]).unwrap(), 0).unwrap()[a[1].parse::<usize>().unwrap()];
    println!("{}", b.text);
    println!("{}", b.char_styles.iter().map(|s| s.to_string()).collect::<String>());
    for s in &b.styles { println!("{} {}", s.font, s.size); }
}
