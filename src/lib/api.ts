// Typed wrappers around the Rust commands (src-tauri/src/commands.rs).
import { invoke } from '@tauri-apps/api/core'

export interface PageInfo {
  width: number
  height: number
}

export interface DocMeta {
  id: string
  name: string
  path: string | null
  dirty: boolean
  info: { page_count: number; pages: PageInfo[]; title: string | null }
}

export interface FontUsage {
  raw_name: string
  family: string
  embedded: boolean
  bold: boolean
  italic: boolean
  pages: number[]
}

export type FontSource =
  | { kind: 'system'; path: string }
  | { kind: 'downloaded'; path: string }
  | { kind: 'substitute'; family: string; path: string; same_metrics: boolean }
  | { kind: 'missing' }

export interface FontResolution {
  raw_name: string
  family: string
  weight: number
  italic: boolean
  embedded: boolean
  source: FontSource
}

export interface TextBlock {
  objects: number[]
  ordinals: number[]
  text: string
  lines: number
  left: number
  bottom: number
  right: number
  top: number
  x: number
  y: number
  size: number
  line_height: number
  font: string
  color: [number, number, number, number]
}

export type EditMode = 'exact' | 'shift_below'

export interface EditOutcome {
  doc: DocMeta
  lossless: boolean
  extra_lines: number
  font: string
}

export interface AssemblePage {
  doc: string
  page: number
  rotate?: number
}

export const api = {
  openFiles: (paths: string[]) =>
    invoke<{ opened: DocMeta[]; errors: string[] }>('open_files', { paths }),
  closeDoc: (id: string) => invoke<void>('close_doc', { id }),
  renderPage: (id: string, page: number, width: number) =>
    invoke<ArrayBuffer>('render_page', { id, page, width }),
  mergeDocs: (ids: string[], name: string) => invoke<DocMeta>('merge_docs', { ids, name }),
  assemblePages: (pages: AssemblePage[], name: string, replace?: string) =>
    invoke<DocMeta>('assemble_pages', { pages, name, replace: replace ?? null }),
  saveDoc: (id: string, path: string) => invoke<string>('save_doc', { id, path }),
  startupFiles: () => invoke<string[]>('startup_files'),
  resolveFonts: (id: string, online: boolean) =>
    invoke<FontResolution[]>('resolve_fonts', { id, online }),
  undo: (id: string) => invoke<DocMeta | null>('undo', { id }),
  pageBlocks: (id: string, page: number) => invoke<TextBlock[]>('page_blocks', { id, page }),
  editBlock: (id: string, page: number, block: TextBlock, text: string, mode: EditMode, online: boolean) =>
    invoke<EditOutcome>('edit_block', { id, page, block, text, mode, online }),
  docFonts: (id: string) => invoke<FontUsage[]>('doc_fonts', { id }),
}

const PDF_FILTER = [{ name: 'PDF', extensions: ['pdf'] }]

export async function pickPdfPaths(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const sel = await open({ multiple: true, filters: PDF_FILTER })
  if (!sel) return []
  return Array.isArray(sel) ? sel : [sel]
}

export async function pickSavePath(suggested: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save({ defaultPath: suggested, filters: PDF_FILTER })
}
