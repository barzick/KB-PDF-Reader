mod commands;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .manage(commands::Library::default())
    .manage(commands::Fonts::default())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      // PDFium ships inside the app bundle, in the "pdfium" resource folder.
      // KB_PDFIUM_DIR overrides it (development).
      let dir = std::env::var("KB_PDFIUM_DIR")
        .map(std::path::PathBuf::from)
        .ok()
        .or_else(|| app.path().resource_dir().ok().map(|d| d.join("pdfium")));
      if let Err(e) = kb_core::init(dir.as_deref()) {
        log::error!("{e}");
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      commands::open_files,
      commands::close_doc,
      commands::render_page,
      commands::merge_docs,
      commands::assemble_pages,
      commands::save_doc,
      commands::doc_fonts,
      commands::startup_files,
      commands::resolve_fonts,
      commands::undo,
      commands::page_blocks,
      commands::edit_block,
    ])
    .run(tauri::generate_context!())
    .expect("error while running KB PDF Reader");
}
