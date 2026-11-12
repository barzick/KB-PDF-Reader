//! Bridge between the window (TypeScript) and the PDF engine (kb-core).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use kb_core::{DocInfo, FontUsage, PageRef};
use parking_lot::RwLock;
use serde::Serialize;
use tauri::ipc::Response;
use tauri::State;

pub struct Doc {
  name: String,
  path: Option<PathBuf>,
  bytes: Arc<Vec<u8>>,
}

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
    self.docs.write().insert(id, Doc { name, path, bytes: Arc::new(bytes) });
    Ok(meta)
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
    let info = kb_core::info(&built).map_err(|e| e.to_string())?;
    let mut docs = lib.docs.write();
    let doc = docs.get_mut(&id).ok_or("document fermé")?;
    doc.bytes = Arc::new(built);
    return Ok(DocMeta {
      id,
      name: doc.name.clone(),
      path: doc.path.as_ref().map(|p| p.display().to_string()),
      dirty: true,
      info,
    });
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
