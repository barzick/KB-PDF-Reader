//! Finds a complete, editable font for every font used in a PDF.
//!
//! Order: fonts on this computer (and our download cache) → free font
//! downloaded from Fontsource (Google Fonts and other open-licence fonts) →
//! metric-compatible free equivalent for commercial fonts → generic fallback.
//! A font embedded in the PDF is often a subset (only the letters already
//! used), so even then we look for the full font to be able to type new text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fontdb::{Database, Family, Query, Stretch, Style, Weight};
use serde::Serialize;

use super::FontUsage;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FontSource {
    /// Installed on this computer.
    System { path: String },
    /// Downloaded earlier or just now from the internet (free licence).
    Downloaded { path: String },
    /// The real font is not freely available: a free font with the same
    /// letter widths is used so the layout does not move.
    Substitute { family: String, path: String, same_metrics: bool },
    /// Nothing usable found (offline and not installed).
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct Resolution {
    pub raw_name: String,
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub embedded: bool,
    pub source: FontSource,
}

/// Downloads a URL and returns its bytes. Swappable for tests.
pub type Fetcher = Box<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

pub struct FontResolver {
    db: Database,
    cache_dir: PathBuf,
    online: bool,
    fetch: Fetcher,
    /// Families we already failed to download this session.
    unavailable: HashMap<String, ()>,
}

/// Commercial fonts → free font with identical metrics (or the closest free design).
/// (target family, same metrics?)
const SUBSTITUTES: &[(&str, &str, bool)] = &[
    ("arial", "Arimo|Liberation Sans", true),
    ("helvetica", "Arimo|Liberation Sans", true),
    ("helvetica neue", "Arimo|Liberation Sans", true),
    ("arial nova", "Arimo|Liberation Sans", true),
    ("liberation sans", "Arimo|Liberation Sans", true),
    ("times new roman", "Tinos|Liberation Serif", true),
    ("times", "Tinos|Liberation Serif", true),
    ("liberation serif", "Tinos|Liberation Serif", true),
    ("courier new", "Cousine|Liberation Mono", true),
    ("courier", "Cousine|Liberation Mono", true),
    ("liberation mono", "Cousine|Liberation Mono", true),
    ("calibri", "Carlito", true),
    ("cambria", "Caladea", true),
    ("georgia", "Gelasio", true),
    ("verdana", "DejaVu Sans", false),
    ("tahoma", "DejaVu Sans", false),
    ("segoe ui", "Open Sans", false),
    ("century gothic", "Questrial", false),
    ("futura", "Jost", false),
    ("garamond", "EB Garamond", false),
    ("book antiqua", "TeX Gyre Pagella", false),
    ("palatino", "TeX Gyre Pagella", false),
    ("palatino linotype", "TeX Gyre Pagella", false),
    ("gill sans", "Cabin", false),
    ("franklin gothic", "Libre Franklin", false),
    ("myriad", "PT Sans", false),
    ("minion", "Crimson Text", false),
    ("symbol", "DejaVu Sans", false),
    ("zapf dingbats", "DejaVu Sans", false),
    ("consolas", "Inconsolata", false),
    ("lucida console", "Cousine", false),
    ("trebuchet", "Fira Sans", false),
    ("trebuchet ms", "Fira Sans", false),
];

/// Fonts that are commercial: never try to download them under their own name.
fn is_commercial(family_lower: &str) -> bool {
    SUBSTITUTES.iter().any(|(k, _, _)| *k == family_lower)
}

fn substitute_for(family_lower: &str) -> Option<(&'static str, bool)> {
    SUBSTITUTES
        .iter()
        .find(|(k, _, _)| *k == family_lower)
        .map(|(_, f, m)| (*f, *m))
}

/// Last resort when we know nothing about the font: guess from its name.
fn generic_fallback(family_lower: &str) -> &'static str {
    if ["mono", "code", "courier", "console", "typewriter"].iter().any(|k| family_lower.contains(k)) {
        "Cousine|Liberation Mono"
    } else if ["serif", "times", "roman", "garamond", "book", "antiqua", "georgia"]
        .iter()
        .any(|k| family_lower.contains(k) && !family_lower.contains("sans"))
    {
        "Tinos|Liberation Serif"
    } else {
        "Arimo|Liberation Sans"
    }
}

fn family_key(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Fontsource id: "Open Sans" → "open-sans".
pub fn fontsource_id(family: &str) -> String {
    family
        .trim()
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn fontsource_url(family: &str, weight: u16, italic: bool) -> String {
    format!(
        "https://cdn.jsdelivr.net/fontsource/fonts/{}@latest/latin-{}-{}.ttf",
        fontsource_id(family),
        weight,
        if italic { "italic" } else { "normal" }
    )
}

fn default_fetcher() -> Fetcher {
    Box::new(|url: &str| {
        let resp = ureq::get(url)
            .timeout(std::time::Duration::from_secs(20))
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader().take(30 * 1024 * 1024), &mut buf)
            .map_err(|e| e.to_string())?;
        // A real TrueType/OpenType file starts with one of these tags.
        let ok = buf.len() > 1024
            && matches!(&buf[..4], [0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf");
        if ok { Ok(buf) } else { Err("réponse inattendue (pas une police)".into()) }
    })
}

use std::io::Read as _;

impl FontResolver {
    /// `cache_dir` keeps downloaded fonts between sessions.
    pub fn new(cache_dir: impl Into<PathBuf>, online: bool) -> Self {
        Self::with_fetcher(cache_dir, online, true, default_fetcher())
    }

    pub fn with_fetcher(cache_dir: impl Into<PathBuf>, online: bool, load_system: bool, fetch: Fetcher) -> Self {
        let cache_dir = cache_dir.into();
        let _ = std::fs::create_dir_all(&cache_dir);
        let mut db = Database::new();
        if load_system {
            db.load_system_fonts();
        }
        db.load_fonts_dir(&cache_dir);
        Self { db, cache_dir, online, fetch, unavailable: HashMap::new() }
    }

    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }

    /// Adds fonts from a folder (e.g. fonts shipped with the app).
    pub fn add_dir(&mut self, dir: &Path) {
        self.db.load_fonts_dir(dir);
    }

    /// Real family name in the database matching `family`, ignoring case, spaces
    /// and dashes ("Deja Vu Serif" and "DejaVuSerif" both find "DejaVu Serif").
    fn installed_family(&self, family: &str) -> Option<String> {
        let key = family_key(family);
        self.db
            .faces()
            .flat_map(|f| f.families.iter())
            .find(|(n, _)| family_key(n) == key)
            .map(|(n, _)| n.clone())
    }

    fn find_local(&self, family: &str, weight: u16, italic: bool) -> Option<(String, bool)> {
        let name = self.installed_family(family)?;
        let q = Query {
            families: &[Family::Name(&name)],
            weight: Weight(weight),
            stretch: Stretch::Normal,
            style: if italic { Style::Italic } else { Style::Normal },
        };
        let face = self.db.face(self.db.query(&q)?)?;
        match &face.source {
            fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => {
                let downloaded = p.starts_with(&self.cache_dir);
                Some((p.display().to_string(), downloaded))
            }
            fontdb::Source::Binary(_) => None,
        }
    }

    /// Downloads a family/weight/style into the cache. Tries the exact weight, then regular.
    fn download(&mut self, family: &str, weight: u16, italic: bool) -> Option<String> {
        if !self.online {
            return None;
        }
        let key = format!("{}-{}-{}", fontsource_id(family), weight, italic);
        if self.unavailable.contains_key(&key) {
            return None;
        }
        let mut tries = vec![(weight, italic)];
        if italic {
            tries.push((weight, false));
        }
        if weight != 400 {
            tries.push((400, false));
        }
        for (w, it) in tries {
            let url = fontsource_url(family, w, it);
            if let Ok(bytes) = (self.fetch)(&url) {
                let file = self.cache_dir.join(format!(
                    "{}-{}-{}.ttf",
                    fontsource_id(family),
                    w,
                    if it { "italic" } else { "normal" }
                ));
                if std::fs::write(&file, &bytes).is_ok() {
                    self.db.load_font_file(&file).ok()?;
                    return Some(file.display().to_string());
                }
            }
        }
        self.unavailable.insert(key, ());
        None
    }

    pub fn resolve(&mut self, usage: &FontUsage) -> Resolution {
        // Show the real name when the font is installed ("DejaVu Serif", not "Deja Vu Serif").
        let family = self.installed_family(&usage.family).unwrap_or_else(|| usage.family.clone());
        let lower = family.to_lowercase();
        let weight = usage.weight.max(if usage.bold { 700 } else { 400 });
        let italic = usage.italic || usage.raw_name.to_lowercase().contains("italic") || usage.raw_name.to_lowercase().contains("oblique");
        let done = |source| Resolution {
            raw_name: usage.raw_name.clone(),
            family: family.clone(),
            weight,
            italic,
            embedded: usage.embedded,
            source,
        };

        // 1. Installed on this computer (or downloaded before).
        if let Some((path, downloaded)) = self.find_local(&family, weight, italic) {
            return done(if downloaded { FontSource::Downloaded { path } } else { FontSource::System { path } });
        }
        // 2. Free font available online under its own name.
        if !is_commercial(&lower) {
            if let Some(path) = self.download(&family, weight, italic) {
                return done(FontSource::Downloaded { path });
            }
        }
        // 3. Known equivalent, then 4. generic guess.
        let (subs, same_metrics) = substitute_for(&lower).unwrap_or((generic_fallback(&lower), false));
        // Prefer any alternative already on this computer, then download the first one.
        let alternatives: Vec<&str> = subs.split('|').collect();
        for sub in &alternatives {
            if let Some((path, _)) = self.find_local(sub, weight, italic) {
                return done(FontSource::Substitute { family: sub.to_string(), path, same_metrics });
            }
        }
        if let Some(path) = self.download(alternatives[0], weight, italic) {
            return done(FontSource::Substitute { family: alternatives[0].to_string(), path, same_metrics });
        }
        done(FontSource::Missing)
    }

    pub fn resolve_all(&mut self, usages: &[FontUsage]) -> Vec<Resolution> {
        usages.iter().map(|u| self.resolve(u)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn usage(raw: &str) -> FontUsage {
        FontUsage {
            raw_name: raw.into(),
            family: crate::normalize_font_name(raw),
            embedded: false,
            bold: false,
            italic: false,
            weight: crate::weight_from_name(raw),
            pages: vec![1],
        }
    }

    /// A real TrueType file to "download" in tests.
    fn sample_ttf() -> Vec<u8> {
        let mut db = Database::new();
        db.load_system_fonts();
        for face in db.faces() {
            if let fontdb::Source::File(p) = &face.source {
                if p.extension().is_some_and(|e| e == "ttf") {
                    return std::fs::read(p).unwrap();
                }
            }
        }
        panic!("aucune police .ttf sur cette machine pour le test");
    }

    fn resolver(online: bool, log: Arc<Mutex<Vec<String>>>, serve: &'static [&'static str]) -> (FontResolver, tempdir::Dir) {
        let dir = tempdir::Dir::new();
        let ttf = sample_ttf();
        let fetch: Fetcher = Box::new(move |url: &str| {
            log.lock().unwrap().push(url.to_string());
            if serve.iter().any(|id| url.contains(&format!("/{id}@"))) { Ok(ttf.clone()) } else { Err("404".into()) }
        });
        (FontResolver::with_fetcher(dir.path(), online, false, fetch), dir)
    }

    #[test]
    fn ids_and_urls() {
        assert_eq!(fontsource_id("Open Sans"), "open-sans");
        assert_eq!(fontsource_id("EB Garamond"), "eb-garamond");
        assert_eq!(
            fontsource_url("Open Sans", 700, true),
            "https://cdn.jsdelivr.net/fontsource/fonts/open-sans@latest/latin-700-italic.ttf"
        );
    }

    #[test]
    fn downloads_free_font_and_caches_it() {
        let log = Arc::new(Mutex::new(vec![]));
        let (mut r, _d) = resolver(true, log.clone(), &["open-sans"]);
        let res = r.resolve(&usage("ABCDEF+OpenSans-Bold"));
        assert!(matches!(res.source, FontSource::Downloaded { .. }), "{res:?}");
        assert!(log.lock().unwrap()[0].contains("open-sans@latest/latin-700-normal.ttf"));
    }

    #[test]
    fn commercial_font_uses_metric_equivalent_without_asking_for_original() {
        let log = Arc::new(Mutex::new(vec![]));
        let (mut r, _d) = resolver(true, log.clone(), &["arimo"]);
        let res = r.resolve(&usage("Arial-BoldMT"));
        match res.source {
            FontSource::Substitute { family, same_metrics, .. } => {
                assert_eq!(family, "Arimo");
                assert!(same_metrics);
            }
            other => panic!("{other:?}"),
        }
        assert!(log.lock().unwrap().iter().all(|u| !u.contains("/arial@")));
    }

    #[test]
    fn unknown_font_falls_back_and_offline_is_missing() {
        let log = Arc::new(Mutex::new(vec![]));
        let (mut r, _d) = resolver(true, log.clone(), &["tinos"]);
        let res = r.resolve(&usage("CompanySerifDisplay-Regular"));
        assert!(matches!(res.source, FontSource::Substitute { ref family, same_metrics: false, .. } if family == "Tinos"), "{res:?}");

        let log2 = Arc::new(Mutex::new(vec![]));
        let (mut off, _d2) = resolver(false, log2.clone(), &["open-sans"]);
        assert_eq!(off.resolve(&usage("OpenSans")).source, FontSource::Missing);
        assert!(log2.lock().unwrap().is_empty(), "hors ligne : aucun téléchargement");
    }

    /// Minimal temp dir helper (avoids an extra dependency).
    mod tempdir {
        pub struct Dir(std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let p = std::env::temp_dir().join(format!(
                    "kb-fonts-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
