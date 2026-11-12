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
    let usage = kb_core::fonts(&bytes)
      .ok()
      .and_then(|all| all.into_iter().find(|f| f.raw_name == block.font))
      .unwrap_or_else(|| FontUsage {
        family: kb_core::normalize_font_name(&block.font),
        raw_name: block.font.clone(),
        embedded: false,
        bold: kb_core::weight_from_name(&block.font) >= 600,
        italic: block.font.to_lowercase().contains("italic"),
        weight: kb_core::weight_from_name(&block.font),
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
    let font = match &path {
      Some(p) => WriteFont::File(std::path::Path::new(p)),
      None => WriteFont::Standard { bold: resolution.weight >= 600, italic: resolution.italic, family },
    };
    kb_core::replace_block(&bytes, page, &block, &text, font, mode)
      .map(|r| (r, label))
      .map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| e.to_string())??;

  let doc = lib.update(&id, result.bytes)?;
  Ok(EditOutcome { doc, lossless: result.lossless, extra_lines: result.extra_lines, font: font_label })
}
