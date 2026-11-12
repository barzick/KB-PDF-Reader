// Typed wrappers around the Rust commands (src-tauri/src/commands.rs).
import { invoke } from '@tauri-apps/api/core'

export interface PageInfo {
  width: number
  height: number
  /** Bottom-left corner of the visible area, in page coordinates. */
  x0: number
  y0: number
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
  /** Distinct styles (font, size, colour) used in the paragraph. */
  styles: { font: string; size: number; color: [number, number, number, number] }[]
  /** Style index of each character of `text`. */
  char_styles: number[]
  justified: boolean
  /** Text recognised by OCR on a scanned page. */
  scanned: boolean
}

export type EditMode = 'exact' | 'shift_below'

export interface EditOutcome {
  doc: DocMeta
  lossless: boolean
  extra_lines: number
  font: string
}

export interface OcrStatus {
  /** Installed OCR languages (fra, eng, deu, ita…). */
  languages: string[]
  /** Pages that look scanned (1-based). */
  scanned: number[]
}

export interface OcrSummary {
  pages: number[]
  skipped: number[]
  words: number
  confidence: number
}

export interface PageText {
  text: string
  /** [left, bottom, right, top] in PDF points for each character (zeros for inferred spaces). */
  boxes: [number, number, number, number][]
}

export interface SearchHit {
  page: number
  rects: [number, number, number, number][]
  context: string
}

export type Rgb = [number, number, number]
export type Rect = [number, number, number, number]

export type NewAnnot =
  | { kind: 'highlight' | 'underline' | 'strike_out'; rects: Rect[]; color: Rgb }
  | { kind: 'note'; x: number; y: number; text: string; color: Rgb }
  | { kind: 'free_text'; rect: Rect; text: string; size: number; color: Rgb; border: boolean }
  | { kind: 'square' | 'circle'; rect: Rect; color: Rgb; width: number; fill: Rgb | null }
  | { kind: 'line'; from: [number, number]; to: [number, number]; color: Rgb; width: number; arrow: boolean }
  | { kind: 'ink'; strokes: [number, number][][]; color: Rgb; width: number }
  | { kind: 'stamp'; rect: Rect; label: string; color: Rgb }
  | { kind: 'image'; rect: Rect; data: number[] }

export interface TextStyle {
  family: 'sans' | 'serif' | 'mono'
  bold: boolean
  italic: boolean
  size: number
  color: Rgb
  opacity: number
}

export interface Watermark {
  text: string
  style: TextStyle
  angle: number
  pages: number[] | null
}

export interface HeaderFooter {
  header: [string, string, string]
  footer: [string, string, string]
  style: TextStyle
  margin: number
  start: number
  pages: number[] | null
  date: string
  file: string
}

/** "1-3, 5" → [0, 1, 2, 4] (0-based); null when empty. Throws on bad input. */
export function parsePages(spec: string, count: number): number[] | null {
  if (!spec.trim()) return null
  const out = new Set<number>()
  for (const part of spec.replace(/\s*-\s*/g, "-").split(/[,;\s]+/).filter(Boolean)) {
    const m = /^(\d+)(?:-(\d+))?$/.exec(part)
    if (!m) throw new Error(`Pages : « ${part} » n'est pas compris`)
    const a = Number(m[1])
    const b = m[2] ? Number(m[2]) : a
    for (let i = Math.min(a, b); i <= Math.max(a, b); i++) if (i >= 1 && i <= count) out.add(i - 1)
  }
  return [...out].sort((x, y) => x - y)
}

export interface Bookmark {
  title: string
  page: number | null
  children: Bookmark[]
}

export interface ProtectOptions {
  user_password: string
  owner_password: string
  allow_print: boolean
  allow_copy: boolean
  allow_modify: boolean
}

export interface ProtectionInfo {
  protected: boolean
  has_password: boolean
  allow_print: boolean
  allow_copy: boolean
  allow_modify: boolean
}

export type CompressLevel = 'lossless' | 'light' | 'medium' | 'strong'

export type Crop =
  | { mode: 'margins'; margins: [number, number, number, number] }
  | { mode: 'content'; pad: number }
  | { mode: 'reset' }

export type FieldKind = 'text' | 'checkbox' | 'radio' | 'combo' | 'list' | 'button' | 'signature'

export interface FormField {
  name: string
  kind: FieldKind
  page: number
  rect: Rect
  value: string
  export: string
  checked: boolean
  options: string[]
  multiline: boolean
  readonly: boolean
  max_len: number | null
  font_size: number
}

export interface AnnotInfo {
  index: number
  subtype: string
  rect: Rect
  contents: string
  color: Rgb | null
}

export type PageFormat = 'a4' | 'a3' | 'a5' | 'letter' | 'legal' | 'fit'

export interface AssemblePage {
  doc: string
  page: number
  rotate?: number
}

export const api = {
  openFiles: (paths: string[]) =>
    invoke<{ opened: DocMeta[]; errors: string[]; locked: string[] }>('open_files', { paths }),
  openLocked: (path: string, password: string) => invoke<DocMeta>('open_locked', { path, password }),
  docProtection: (id: string) => invoke<ProtectionInfo>('doc_protection', { id }),
  setProtection: (id: string, protect: ProtectOptions | null) => invoke<DocMeta>('set_protection', { id, protect }),
  compressDoc: (id: string, level: CompressLevel) =>
    invoke<{ doc: DocMeta; result: { before: number; after: number; images: number } }>('compress_doc', { id, level }),
  cropDoc: (id: string, pages: number[] | null, crop: Crop) => invoke<DocMeta>('crop_doc', { id, pages, crop }),
  splitDoc: (id: string, groups: number[][], dir: string, base: string) => invoke<string[]>('split_doc', { id, groups, dir, base }),
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
  ocrStatus: (id: string) => invoke<OcrStatus>('ocr_status', { id }),
  ocrDoc: (id: string, languages: string[], pages: number[] | null) =>
    invoke<{ doc: DocMeta | null; summary: OcrSummary }>('ocr_doc', { id, languages, pages }),
  createFromImages: (paths: string[], format: PageFormat, margin: number) =>
    invoke<DocMeta>('create_from_images', { paths, format, margin }),
  createBlank: (count: number, format: PageFormat, landscape: boolean) =>
    invoke<DocMeta>('create_blank', { count, format, landscape }),
  insertBlank: (id: string, at: number) => invoke<DocMeta>('insert_blank', { id, at }),
  addText: (id: string, page: number, x: number, y: number, text: string, style: TextStyle) =>
    invoke<DocMeta>('add_text', { id, page, x, y, text, style }),
  addWatermark: (id: string, watermark: Watermark) => invoke<DocMeta>('add_watermark', { id, watermark }),
  addHeaderFooter: (id: string, spec: HeaderFooter) => invoke<DocMeta>('add_header_footer', { id, spec }),
  getBookmarks: (id: string) => invoke<Bookmark[]>('get_bookmarks', { id }),
  setBookmarks: (id: string, list: Bookmark[]) => invoke<DocMeta>('set_bookmarks', { id, list }),
  printView: () => invoke<void>('print_view'),
  aiSettings: () => invoke<AiSettings>('ai_settings'),
  aiSaveProfile: (profile: AiProfile, apiKey: string | null) => invoke<AiSettings>('ai_save_profile', { profile, apiKey }),
  aiDeleteProfile: (id: string) => invoke<AiSettings>('ai_delete_profile', { id }),
  aiSetActive: (id: string) => invoke<AiSettings>('ai_set_active', { id }),
  aiListModels: (id: string) => invoke<string[]>('ai_list_models', { id }),
  aiChat: (requestId: string, profileId: string | null, system: string, messages: ChatMessage[]) =>
    invoke<string>('ai_chat', { requestId, profileId, system, messages }),
  aiCancel: (requestId: string) => invoke<void>('ai_cancel', { requestId }),
  aiDocContext: (id: string, question: string, current: number, budget: number) =>
    invoke<DocContext>('ai_doc_context', { id, question, current, budget }),
  compareDocs: (old: string, next: string) => invoke<Comparison>('compare_docs', { old, new: next }),
  batchProcess: (paths: string[], action: BatchAction, dir: string) =>
    invoke<BatchResult[]>('batch_process', { paths, action, dir }),
  convertDoc: (id: string, target: ConvertTarget, pages: number[], dest: string, ocr: boolean, languages: string[]) =>
    invoke<string[]>('convert_doc', { id, target, pages, dest, ocr, languages }),
  listFields: (id: string) => invoke<FormField[]>('list_fields', { id }),
  fillFields: (id: string, values: { name: string; value: string }[]) => invoke<DocMeta>('fill_fields', { id, values }),
  flattenDoc: (id: string) => invoke<DocMeta>('flatten_doc', { id }),
  listAnnots: (id: string, page: number) => invoke<AnnotInfo[]>('list_annots', { id, page }),
  addAnnot: (id: string, page: number, annot: NewAnnot) => invoke<DocMeta>('add_annot', { id, page, annot }),
  deleteAnnot: (id: string, page: number, index: number) => invoke<DocMeta>('delete_annot', { id, page, index }),
  setNote: (id: string, page: number, index: number, text: string) =>
    invoke<DocMeta>('set_note', { id, page, index, text }),
  pageText: (id: string, page: number) => invoke<PageText>('page_text', { id, page }),
  searchDoc: (id: string, query: string, matchCase: boolean) =>
    invoke<SearchHit[]>('search_doc', { id, query, matchCase }),
}

export type ConvertTarget =
  | { kind: 'images'; format: 'png' | 'jpeg'; dpi: number }
  | { kind: 'text' }
  | { kind: 'word' }
  | { kind: 'excel' }

export type BatchAction =
  | { action: 'compress'; level: CompressLevel }
  | { action: 'convert'; target: ConvertTarget }
  | { action: 'ocr'; languages: string[] }
  | { action: 'protect'; options: ProtectOptions }
  | { action: 'watermark'; watermark: Watermark }
  | { action: 'header_footer'; spec: HeaderFooter }

export interface BatchResult {
  file: string
  outputs: string[]
  error: string | null
}

export interface Mark {
  page: number
  rect: [number, number, number, number]
}

export interface Change {
  kind: 'inserted' | 'deleted' | 'replaced'
  old_text: string
  new_text: string
  old_marks: Mark[]
  new_marks: Mark[]
  old_page: number
  new_page: number
}

export interface Comparison {
  changes: Change[]
  old_words: number
  new_words: number
}

export interface AiProfile {
  id: string
  name: string
  kind: 'openai' | 'anthropic'
  endpoint: string
  model: string
  context_chars: number
}

export interface AiSettings {
  profiles: (AiProfile & { has_key: boolean })[]
  active: string | null
}

export interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
}

export interface DocContext {
  text: string
  pages: number[]
  partial: boolean
}

export const IMAGE_EXTENSIONS = ['jpg', 'jpeg', 'png', 'tif', 'tiff', 'webp', 'bmp']

export async function pickImagePaths(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const sel = await open({ multiple: true, filters: [{ name: 'Images', extensions: IMAGE_EXTENSIONS }] })
  if (!sel) return []
  return Array.isArray(sel) ? sel : [sel]
}

const PDF_FILTER = [{ name: 'PDF', extensions: ['pdf'] }]

export async function pickPdfPaths(): Promise<string[]> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const sel = await open({ multiple: true, filters: PDF_FILTER })
  if (!sel) return []
  return Array.isArray(sel) ? sel : [sel]
}

export async function pickFolder(): Promise<string | null> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const sel = await open({ directory: true })
  return typeof sel === 'string' ? sel : null
}

export async function pickSaveAs(suggested: string, name: string, extensions: string[]): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save({ defaultPath: suggested, filters: [{ name, extensions }] })
}

export async function revealFile(path: string): Promise<void> {
  const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
  await revealItemInDir(path)
}

export async function pickSavePath(suggested: string): Promise<string | null> {
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save({ defaultPath: suggested, filters: PDF_FILTER })
}
