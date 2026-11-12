//! Bridge between the window (TypeScript) and the PDF engine (kb-core).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kb_core::{DocInfo, EditMode, FontResolver, FontSource, FontUsage, PageRef, Resolution, StandardFamily, TextBlock, WriteFont};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{Manager, State};

pub struct Doc {
  name: String,
  path: Option<PathBuf>,
  bytes: Arc<Vec<u8>>,
  /// Previous versions, for undo (most recent last).
  history: Vec<Arc<Vec<u8>>>,
}

const MAX_UNDO: usize = 50;

#[derive(Default)]
pub struct Library {
  docs: RwLock<HashMap<String, Doc>>,
  next: AtomicU64,
}

impl Library {
  fn insert(&self, name: String, path: Option<PathBuf>, bytes: Vec<u8>) -> Result<DocMeta, String> {
    let info = kb_core::info(&bytes).map_err(|e| e.to_string())?;
    let id = format!("d{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
    let meta = DocMeta {
      id: id.clone(),
      name: name.clone(),
      path: path.as_ref().map(|p| p.display().to_string()),
      dirty: path.is_none(),
      info,
    };
    self.docs.write().insert(id, Doc { name, path, bytes: Arc::new(bytes), history: vec![] });
    Ok(meta)
  }

  /// Replaces a document's content, keeping the previous version for undo.
  fn update(&self, id: &str, bytes: Vec<u8>) -> Result<DocMeta, String> {
    let info = kb_core::info(&bytes).map_err(|e| e.to_string())?;
    let mut docs = self.docs.write();
    let doc = docs.get_mut(id).ok_or("document fermé")?;
    let old = std::mem::replace(&mut doc.bytes, Arc::new(bytes));
    doc.history.push(old);
    if doc.history.len() > MAX_UNDO {
      doc.history.remove(0);
    }
    Ok(DocMeta {
      id: id.to_string(),
      name: doc.name.clone(),
      path: doc.path.as_ref().map(|p| p.display().to_string()),
      dirty: true,
      info,
    })
  }

  fn bytes(&self, id: &str) -> Result<Arc<Vec<u8>>, String> {
    self.docs.read().get(id).map(|d| d.bytes.clone()).ok_or_else(|| "document fermé".into())
  }
}

#[derive(Serialize)]
pub struct DocMeta {
  id: String,
  name: String,
  path: Option<String>,
  /// True when the document only exists in memory (e.g. a fresh merge).
  dirty: bool,
  info: DocInfo,
}

#[derive(Serialize)]
pub struct OpenResult {
  opened: Vec<DocMeta>,
  errors: Vec<String>,
}

fn file_name(p: &std::path::Path) -> String {
  p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "document.pdf".into())
}

#[tauri::command]
pub async fn open_files(lib: State<'_, Library>, paths: Vec<String>) -> Result<OpenResult, String> {
  let mut out = OpenResult { opened: vec![], errors: vec![] };
  for p in paths {
    let path = PathBuf::from(&p);
    match std::fs::read(&path) {
      Ok(bytes) => match lib.insert(file_name(&path), Some(path.clone()), bytes) {
        Ok(m) => out.opened.push(m),
        Err(e) => out.errors.push(format!("{} : {e}", file_name(&path))),
      },
      Err(e) => out.errors.push(format!("{} : {e}", file_name(&path))),
    }
  }
  Ok(out)
}

#[tauri::command]
pub fn close_doc(lib: State<'_, Library>, id: String) {
  lib.docs.write().remove(&id);
}

/// Returns the page as raw PNG bytes (an ArrayBuffer on the JS side).
#[tauri::command]
pub async fn render_page(lib: State<'_, Library>, id: String, page: u32, width: u32) -> Result<Response, String> {
  let bytes = lib.bytes(&id)?;
  let png = tauri::async_runtime::spawn_blocking(move || kb_core::render_png(&bytes, page, width))
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
  Ok(Response::new(png))
}

#[tauri::command]
pub async fn merge_docs(lib: State<'_, Library>, ids: Vec<String>, name: String) -> Result<DocMeta, String> {
  let sources = ids.iter().map(|id| lib.bytes(id)).collect::<Result<Vec<_>, _>>()?;
  let merged = tauri::async_runtime::spawn_blocking(move || {
    let refs: Vec<&[u8]> = sources.iter().map(|b| b.as_slice()).collect();
    kb_core::merge(&refs)
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;
  lib.insert(name, None, merged)
}

/// Builds a new document from pages of open documents (reorder, delete, extract, rotate).
/// When `replace` is set, the result replaces that document in place instead of opening a new one.
#[derive(serde::Deserialize)]
pub struct AssemblePage {
  doc: String,
  page: u32,
  #[serde(default)]
  rotate: u16,
}

#[tauri::command]
pub async fn assemble_pages(
  lib: State<'_, Library>,
  pages: Vec<AssemblePage>,
  name: String,
  replace: Option<String>,
) -> Result<DocMeta, String> {
  let mut ids: Vec<String> = vec![];
  let mut refs = vec![];
  for p in &pages {
    let source = match ids.iter().position(|i| i == &p.doc) {
      Some(i) => i,
      None => {
        ids.push(p.doc.clone());
        ids.len() - 1
      }
    };
    refs.push(PageRef { source, page: p.page, rotate: p.rotate });
  }
  let sources = ids.iter().map(|id| lib.bytes(id)).collect::<Result<Vec<_>, _>>()?;
  let built = tauri::async_runtime::spawn_blocking(move || {
    let s: Vec<&[u8]> = sources.iter().map(|b| b.as_slice()).collect();
    kb_core::assemble(&s, &refs)
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;

  if let Some(id) = replace {
    return lib.update(&id, built);
  }
  lib.insert(name, None, built)
}

#[tauri::command]
pub async fn save_doc(lib: State<'_, Library>, id: String, path: String) -> Result<String, String> {
  let bytes = lib.bytes(&id)?;
  let path = PathBuf::from(path);
  std::fs::write(&path, bytes.as_slice()).map_err(|e| e.to_string())?;
  let name = file_name(&path);
  if let Some(d) = lib.docs.write().get_mut(&id) {
    d.name = name.clone();
    d.path = Some(path);
  }
  Ok(name)
}

#[tauri::command]
pub async fn doc_fonts(lib: State<'_, Library>, id: String) -> Result<Vec<FontUsage>, String> {
  let bytes = lib.bytes(&id)?;
  kb_core::fonts(&bytes).map_err(|e| e.to_string())
}

/// PDF paths passed on the command line (e.g. "Open with KB PDF Reader" on Windows).
#[tauri::command]
pub fn startup_files() -> Vec<String> {
  std::env::args()
    .skip(1)
    .filter(|a| a.to_lowercase().ends_with(".pdf") && std::path::Path::new(a).is_file())
    .collect()
}

/// Font resolver shared by all documents; created on first use.
#[derive(Default)]
pub struct Fonts(Arc<Mutex<Option<FontResolver>>>);

/// Finds a complete font for each font of the document:
/// this computer → free download (cached) → equivalent with same widths.
#[tauri::command]
pub async fn resolve_fonts(
  app: tauri::AppHandle,
  lib: State<'_, Library>,
  fonts: State<'_, Fonts>,
  id: String,
  online: bool,
) -> Result<Vec<Resolution>, String> {
  let bytes = lib.bytes(&id)?;
  let cache = app.path().app_data_dir().map_err(|e| e.to_string())?.join("fonts");
  let shared = fonts.0.clone();
  tauri::async_runtime::spawn_blocking(move || {
    let usages = kb_core::fonts(&bytes).map_err(|e| e.to_string())?;
    let mut guard = shared.lock();
    let resolver = guard.get_or_insert_with(|| FontResolver::new(&cache, online));
    resolver.set_online(online);
    Ok(resolver.resolve_all(&usages))
  })
  .await
  .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn undo(lib: State<'_, Library>, id: String) -> Result<Option<DocMeta>, String> {
  let mut docs = lib.docs.write();
  let doc = docs.get_mut(&id).ok_or("document fermé")?;
  let Some(prev) = doc.history.pop() else { return Ok(None) };
  doc.bytes = prev;
  let info = kb_core::info(&doc.bytes).map_err(|e| e.to_string())?;
  Ok(Some(DocMeta {
    id,
    name: doc.name.clone(),
    path: doc.path.as_ref().map(|p| p.display().to_string()),
    dirty: true,
    info,
  }))
}

/// Paragraphs of a page, for the text editing mode.
#[tauri::command]
pub async fn page_blocks(lib: State<'_, Library>, id: String, page: u32) -> Result<Vec<TextBlock>, String> {
  let bytes = lib.bytes(&id)?;
  tauri::async_runtime::spawn_blocking(move || kb_core::text_blocks(&bytes, page))
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct EditOutcome {
  doc: DocMeta,
  /// The rest of the page was kept exactly as it was.
  lossless: bool,
  extra_lines: i32,
  /// Font actually used to write, e.g. "Arial" or "Arimo (remplace Arial)".
  font: String,
}

/// Rewrites one paragraph. The font is found by the resolver (computer →
/// download → equivalent); the 14 standard PDF fonts are the last resort.
#[tauri::command]
pub async fn edit_block(
  app: tauri::AppHandle,
  lib: State<'_, Library>,
  fonts: State<'_, Fonts>,
  id: String,
  page: u32,
  block: TextBlock,
  text: String,
  mode: EditMode,
  online: bool,
) -> Result<EditOutcome, String> {
  let bytes = lib.bytes(&id)?;
  let cache = app.path().app_data_dir().map_err(|e| e.to_string())?.join("fonts");
  let shared = fonts.0.clone();
  let (result, font_label) = tauri::async_runtime::spawn_blocking(move || {
    let used = kb_core::fonts(&bytes).unwrap_or_default();
    // One font per style run (bold, italic… parts of the paragraph).
    let mut names: Vec<String> = block.styles.iter().map(|s| s.font.clone()).collect();
    if names.is_empty() {
      names.push(block.font.clone());
    }
    let mut picks: Vec<(Option<String>, bool, bool, StandardFamily, String)> = Vec::new();
    for name in &names {
      let usage = used.iter().find(|f| &f.raw_name == name).cloned().unwrap_or_else(|| FontUsage {
        family: kb_core::normalize_font_name(name),
        raw_name: name.clone(),
        embedded: false,
        bold: kb_core::weight_from_name(name) >= 600,
        italic: name.to_lowercase().contains("italic") || name.to_lowercase().contains("oblique"),
        weight: kb_core::weight_from_name(name),
        pages: vec![page + 1],
      });
      let resolution = {
        let mut guard = shared.lock();
        let resolver = guard.get_or_insert_with(|| FontResolver::new(&cache, online));
        resolver.set_online(online);
        resolver.resolve(&usage)
      };
      let (path, label) = match &resolution.source {
        FontSource::System { path } | FontSource::Downloaded { path } => (Some(path.clone()), resolution.family.clone()),
        FontSource::Substitute { family, path, .. } => (Some(path.clone()), format!("{family} (remplace {})", resolution.family)),
        FontSource::Missing => (None, format!("police standard (remplace {})", resolution.family)),
      };
      let lower = resolution.family.to_lowercase();
      let family = if lower.contains("mono") || lower.contains("courier") {
        StandardFamily::Mono
      } else if lower.contains("times") || lower.contains("serif") && !lower.contains("sans") {
        StandardFamily::Serif
      } else {
        StandardFamily::Sans
      };
      picks.push((path, resolution.weight >= 600, resolution.italic, family, label));
    }
    let mut labels: Vec<String> = Vec::new();
    for p in &picks {
      if !labels.contains(&p.4) {
        labels.push(p.4.clone());
      }
    }
    let label = labels.join(", ");
    let fonts: Vec<WriteFont> = picks
      .iter()
      .map(|(path, bold, italic, family, _)| match path {
        Some(p) => WriteFont::File(std::path::Path::new(p)),
        None => WriteFont::Standard { bold: *bold, italic: *italic, family: *family },
      })
      .collect();
    kb_core::replace_block(&bytes, page, &block, &text, fonts, mode)
      .map(|r| (r, label))
      .map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| e.to_string())??;

  let doc = lib.update(&id, result.bytes)?;
  Ok(EditOutcome { doc, lossless: result.lossless, extra_lines: result.extra_lines, font: font_label })
}

// ---------------------------------------------------------------- OCR

/// The OCR language models ship inside the app ("tessdata" resource folder).
/// KB_TESSDATA_DIR overrides it (development).
fn tessdata_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
  std::env::var("KB_TESSDATA_DIR")
    .map(PathBuf::from)
    .ok()
    .or_else(|| app.path().resource_dir().ok().map(|d| d.join("tessdata")))
    .filter(|d| d.is_dir())
    .ok_or_else(|| "modèles OCR introuvables".to_string())
}

#[derive(Serialize)]
pub struct OcrStatus {
  /// Installed languages (Tesseract codes: fra, eng, deu, ita…).
  languages: Vec<String>,
  /// Pages that look scanned (no text), 1-based.
  scanned: Vec<u32>,
}

#[tauri::command]
pub async fn ocr_status(app: tauri::AppHandle, lib: State<'_, Library>, id: String) -> Result<OcrStatus, String> {
  let bytes = lib.bytes(&id)?;
  let languages = tessdata_dir(&app).map(|d| kb_core::ocr_languages(&d)).unwrap_or_default();
  let scanned = tauri::async_runtime::spawn_blocking(move || kb_core::scanned_pages(&bytes))
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
  Ok(OcrStatus { languages, scanned: scanned.into_iter().map(|p| p + 1).collect() })
}

#[derive(Serialize, Clone)]
struct OcrProgress {
  id: String,
  done: usize,
  total: usize,
}

#[derive(Serialize)]
pub struct OcrOutcome {
  doc: Option<DocMeta>,
  summary: kb_core::OcrSummary,
}

/// Recognises the text of scanned pages (1-based `pages`, all when empty) and
/// adds it as an invisible, searchable and editable layer.
#[tauri::command]
pub async fn ocr_doc(
  app: tauri::AppHandle,
  lib: State<'_, Library>,
  id: String,
  languages: Vec<String>,
  pages: Option<Vec<u32>>,
) -> Result<OcrOutcome, String> {
  use tauri::Emitter;
  let bytes = lib.bytes(&id)?;
  let tessdata = tessdata_dir(&app)?;
  let langs = if languages.is_empty() { "fra+eng".to_string() } else { languages.join("+") };
  let emitter = app.clone();
  let doc_id = id.clone();
  let (out, summary) = tauri::async_runtime::spawn_blocking(move || {
    let pages: Option<Vec<u32>> = pages.map(|p| p.into_iter().filter(|&n| n > 0).map(|n| n - 1).collect());
    kb_core::make_searchable(&bytes, pages.as_deref(), &tessdata, &langs, |done, total| {
      let _ = emitter.emit("ocr-progress", OcrProgress { id: doc_id.clone(), done, total });
    })
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;
  let doc = if summary.pages.is_empty() { None } else { Some(lib.update(&id, out)?) };
  Ok(OcrOutcome { doc, summary })
}

// ---------------------------------------------------------------- creation

#[tauri::command]
pub async fn create_from_images(
  lib: State<'_, Library>,
  paths: Vec<String>,
  format: kb_core::PageFormat,
  margin: f32,
) -> Result<DocMeta, String> {
  let files = paths.iter().map(std::fs::read).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
  let name = match paths.first() {
    Some(p) if paths.len() == 1 => {
      let stem = PathBuf::from(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
      format!("{stem}.pdf")
    }
    _ => "Images.pdf".into(),
  };
  let pdf = tauri::async_runtime::spawn_blocking(move || {
    let refs: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    kb_core::images_to_pdf(&refs, format, margin)
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;
  lib.insert(name, None, pdf)
}

#[tauri::command]
pub async fn create_blank(lib: State<'_, Library>, count: u32, format: kb_core::PageFormat, landscape: bool) -> Result<DocMeta, String> {
  let pdf = kb_core::blank_document(count, format, landscape).map_err(|e| e.to_string())?;
  lib.insert("Nouveau document.pdf".into(), None, pdf)
}

/// Inserts a blank page before page `at` (0-based; the page count appends).
#[tauri::command]
pub async fn insert_blank(lib: State<'_, Library>, id: String, at: u32) -> Result<DocMeta, String> {
  let bytes = lib.bytes(&id)?;
  let out = kb_core::insert_blank_page(&bytes, at).map_err(|e| e.to_string())?;
  lib.update(&id, out)
}
