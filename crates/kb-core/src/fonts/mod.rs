use serde::Serialize;

mod resolve;
pub use resolve::{FontResolver, FontSource, Resolution};

#[derive(Debug, Clone, Serialize)]
pub struct FontUsage {
    /// Name as stored in the PDF, e.g. "ABCDEF+Arial-BoldMT".
    pub raw_name: String,
    /// Clean family name used for lookup, e.g. "Arial".
    pub family: String,
    pub embedded: bool,
    pub bold: bool,
    pub italic: bool,
    /// CSS-style weight guessed from the name (400 regular, 700 bold…).
    pub weight: u16,
    /// 1-based page numbers where the font appears.
    pub pages: Vec<u32>,
}

/// Turns a PDF font name into a family name that can be searched for.
/// "ABCDEF+Arial-BoldMT" → "Arial", "TimesNewRomanPS-ItalicMT" → "Times New Roman",
/// "OpenSans-SemiBold" → "Open Sans".
pub fn normalize_font_name(raw: &str) -> String {
    // Subset prefix: six uppercase letters followed by '+'.
    let mut name = match raw.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => raw,
    };
    // Style suffix after '-' or ',' ("Arial-BoldMT", "Arial,Bold").
    if let Some(i) = name.find(['-', ',']) {
        name = &name[..i];
    }
    let mut name = name.to_string();
    for suffix in ["PSMT", "MT", "PS", "Std", "Pro", "LT"] {
        if name.len() > suffix.len() + 2 && name.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    // Split CamelCase: "TimesNewRoman" → "Times New Roman".
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let prev = chars[i - 1];
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_lowercase() || (prev.is_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(if c == '_' { ' ' } else { c });
    }
    out.trim().to_string()
}

/// Guesses the CSS weight from a PDF font name ("Arial-BoldMT" → 700).
pub fn weight_from_name(raw: &str) -> u16 {
    let n = raw.to_lowercase().replace([' ', '_'], "");
    let table: [(&str, u16); 12] = [
        ("thin", 100), ("hairline", 100), ("extralight", 200), ("ultralight", 200),
        ("light", 300), ("medium", 500), ("semibold", 600), ("demibold", 600),
        ("extrabold", 800), ("ultrabold", 800), ("black", 900), ("heavy", 900),
    ];
    for (k, w) in table {
        if n.contains(k) {
            return w;
        }
    }
    if n.contains("bold") { 700 } else { 400 }
}

#[cfg(test)]
mod tests {
    use super::normalize_font_name as n;

    #[test]
    fn cleans_common_names() {
        assert_eq!(n("ABCDEF+Arial-BoldMT"), "Arial");
        assert_eq!(n("TimesNewRomanPS-ItalicMT"), "Times New Roman");
        assert_eq!(n("OpenSans-SemiBold"), "Open Sans");
        assert_eq!(n("Helvetica"), "Helvetica");
        assert_eq!(n("Calibri,Bold"), "Calibri");
        assert_eq!(n("QWERTY+Roboto-Regular"), "Roboto");
    }

    #[test]
    fn weights() {
        use super::weight_from_name as w;
        assert_eq!(w("Arial-BoldMT"), 700);
        assert_eq!(w("OpenSans-SemiBold"), 600);
        assert_eq!(w("Roboto-Light"), 300);
        assert_eq!(w("Helvetica"), 400);
    }
}
