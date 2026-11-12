import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, IMAGE_EXTENSIONS, pickFolder, pickSaveAs, revealFile, type ConvertTarget, pickImagePaths, pickPdfPaths, pickSavePath, type DocMeta, type OcrStatus, type PageFormat } from './lib/api'
import { PageImage } from './components/PageImage'
import { MergeDialog } from './components/MergeDialog'
import { FontsPanel } from './components/FontsPanel'
import { EditLayer } from './components/EditLayer'
import { OcrDialog } from './components/OcrDialog'
import { NewDialog } from './components/NewDialog'
import { Menu } from './components/Menu'
import { AnnotToolbar, MARKUP_TOOLS, type AnnotTool } from './components/AnnotToolbar'
import { AnnotLayer } from './components/AnnotLayer'
import { lineRects } from './lib/pageText'
import { LicenseDialog, licenceAccepted } from './components/LicenseDialog'
import type { Bookmark, CompressLevel, Crop, EditMode, FormField, HeaderFooter, ProtectOptions, NewAnnot, PageText, Rect, Rgb, SearchHit, TextBlock, TextStyle, Watermark } from './lib/api'
import { TextLayer, type Selection } from './components/TextLayer'
import { SearchBar } from './components/SearchBar'
import { FormLayer } from './components/FormLayer'
import { PlaceLayer } from './components/PlaceLayer'
import { TextPlaceLayer } from './components/TextPlaceLayer'
import { HeaderFooterDialog, WatermarkDialog } from './components/DecorateDialog'
import { BookmarksPanel } from './components/BookmarksPanel'
import { PrintDialog, PrintRoot } from './components/Print'
import { ConvertDialog, type ConvertKind } from './components/ConvertDialog'
import { BatchDialog } from './components/BatchDialog'
import { CompareView } from './components/CompareView'
import { CompressDialog, CropDialog, PasswordDialog, ProtectDialog, SplitDialog } from './components/DocumentDialogs'
import { SignatureDialog, fromFile, type PlacedImage } from './components/SignatureDialog'
import { sliceText } from './lib/pageText'

interface OpenDoc extends DocMeta {
  /** Bumped whenever the document content changes, to refresh page images. */
  version: number
}

const THUMB_W = 120
const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3]

export default function App() {
  const [docs, setDocs] = useState<OpenDoc[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)
  const [zoom, setZoom] = useState(1)
  const [fitWidth, setFitWidth] = useState(true)
  const [current, setCurrent] = useState(0)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [side, setSide] = useState<'pages' | 'bookmarks' | 'fonts'>('pages')
  const [printOpen, setPrintOpen] = useState(false)
  const [convert, setConvert] = useState<ConvertKind | null>(null)
  const [printJob, setPrintJob] = useState<{ pages: { index: number; width: number; height: number }[]; dpi: number } | null>(null)
  const [merging, setMerging] = useState(false)
  const [batchOpen, setBatchOpen] = useState(false)
  const [comparing, setComparing] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [editing, setEditing] = useState(false)
  const [ocr, setOcr] = useState<(OcrStatus & { key: string }) | null>(null)
  const [ocrOpen, setOcrOpen] = useState(false)
  const [newOpen, setNewOpen] = useState(false)
  const [annotating, setAnnotating] = useState(false)
  const [tool, setTool] = useState<AnnotTool>('highlight')
  const [annotColor, setAnnotColor] = useState<Rgb>([255, 230, 0])
  const [annotWidth, setAnnotWidth] = useState(2)
  const [stamp, setStamp] = useState('Approuvé')
  const selectionRef = useRef<{ sel: Selection; text: PageText } | null>(null)
  const [licence, setLicence] = useState<'first' | 'about' | null>(() => (licenceAccepted() ? null : 'first'))
  const [bannerHidden, setBannerHidden] = useState<Set<string>>(new Set())
  const [selection, setSelection] = useState<{ sel: Selection; text: PageText } | null>(null)
  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [matchCase, setMatchCase] = useState(false)
  const [hits, setHits] = useState<SearchHit[]>([])
  const [hitIndex, setHitIndex] = useState(0)
  const [searching, setSearching] = useState(false)
  const [fields, setFields] = useState<FormField[]>([])
  const [signOpen, setSignOpen] = useState<{ page: number; rect: Rect } | true | null>(null)
  const [placing, setPlacing] = useState<{ image: PlacedImage; width: number; what: string } | null>(null)
  const formNoticed = useRef(new Set<string>())
  const [addingText, setAddingText] = useState(false)
  const [textStyle, setTextStyle] = useState<TextStyle>({ family: 'sans', bold: false, italic: false, size: 12, color: [0, 0, 0], opacity: 1 })
  const [decorate, setDecorate] = useState<'watermark' | 'header' | 'numbers' | null>(null)
  const [docDialog, setDocDialog] = useState<'protect' | 'compress' | 'crop' | 'split' | null>(null)
  const [locked, setLocked] = useState<string[]>([])
  const [lockError, setLockError] = useState<string | null>(null)
  const imageInput = useRef<HTMLInputElement>(null)
  const viewer = useRef<HTMLDivElement>(null)
  const [viewerWidth, setViewerWidth] = useState(800)

  const doc = docs.find((d) => d.id === activeId) ?? null

  const notify = useCallback((msg: string) => {
    setToast(msg)
    window.setTimeout(() => setToast((t) => (t === msg ? null : t)), 4000)
  }, [])

  const run = useCallback(
    async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
      setBusy(label)
      try {
        return await fn()
      } catch (e) {
        notify(String(e))
      } finally {
        setBusy(null)
      }
    },
    [notify],
  )

  const addDocs = useCallback((metas: DocMeta[]) => {
    if (metas.length === 0) return
    setDocs((d) => [...d, ...metas.map((m) => ({ ...m, version: 0 }))])
    setActiveId(metas[metas.length - 1].id)
  }, [])

  const openPaths = useCallback(
    async (paths: string[], activate = true) => {
      const res = await run('Ouverture…', () => api.openFiles(paths))
      if (!res) return []
      if (res.errors.length) notify(`Impossible d'ouvrir : ${res.errors.join(' ; ')}`)
      if (res.locked.length) setLocked((l) => [...l, ...res.locked])
      if (activate) addDocs(res.opened)
      else setDocs((d) => [...d, ...res.opened.map((m) => ({ ...m, version: 0 }))])
      return res.opened
    },
    [run, notify, addDocs],
  )

  const openDialog = useCallback(async () => {
    const paths = await pickPdfPaths()
    if (paths.length) await openPaths(paths)
  }, [openPaths])

  // Files given at launch ("Open with…").
  useEffect(() => {
    api.startupFiles().then((p) => {
      if (p.length) openPaths(p)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Files dropped onto the window.
  useEffect(() => {
    let unlisten: (() => void) | undefined
    import('@tauri-apps/api/webview').then(({ getCurrentWebview }) =>
      getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === 'over' || e.payload.type === 'enter') setDragOver(true)
          else if (e.payload.type === 'leave') setDragOver(false)
          else if (e.payload.type === 'drop') {
            setDragOver(false)
            const pdfs = e.payload.paths.filter((p) => p.toLowerCase().endsWith('.pdf'))
            const images = e.payload.paths.filter((p) => IMAGE_EXTENSIONS.includes(p.split('.').pop()?.toLowerCase() ?? ''))
            if (pdfs.length) openPaths(pdfs)
            if (images.length) fromImagesRef.current(images, 'a4', 0)
          }
        })
        .then((u) => (unlisten = u)),
    )
    return () => unlisten?.()
  }, [openPaths])

  // Scanned pages of the active document (offers OCR).
  useEffect(() => {
    if (!doc) return setOcr(null)
    const key = `${doc.id}:${doc.version}`
    let alive = true
    api
      .ocrStatus(doc.id)
      .then((st) => alive && setOcr({ ...st, key }))
      .catch(() => alive && setOcr(null))
    return () => {
      alive = false
    }
  }, [doc?.id, doc?.version])

  // OCR progress, page by page.
  useEffect(() => {
    let unlisten: (() => void) | undefined
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen<{ id: string; done: number; total: number }>('ocr-progress', (e) =>
        setBusy(`Reconnaissance du texte… ${Math.min(e.payload.done + 1, e.payload.total)}/${e.payload.total}`),
      ).then((u) => (unlisten = u)),
    )
    return () => unlisten?.()
  }, [])

  // Conversion progress.
  useEffect(() => {
    let unlisten: (() => void) | undefined
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen<{ stage: 'ocr' | 'convert'; done: number; total: number }>('convert-progress', (e) => {
        const { stage, done, total } = e.payload
        const step = Math.min(done + 1, total)
        if (done >= total && stage === 'convert') return
        setBusy(stage === 'ocr' ? `Reconnaissance du texte… ${step}/${total}` : `Conversion… page ${step}/${total}`)
      }).then((u) => (unlisten = u)),
    )
    return () => unlisten?.()
  }, [])

  // Search (debounced), redone when the document changes.
  useEffect(() => {
    if (!doc || !searchOpen || !query.trim()) {
      setHits([])
      setSearching(false)
      return
    }
    let live = true
    setSearching(true)
    const t = window.setTimeout(() => {
      api
        .searchDoc(doc.id, query, matchCase)
        .then((h) => {
          if (!live) return
          setHits(h)
          setHitIndex(0)
          if (h.length) showHit(h[0])
        })
        .catch(() => live && setHits([]))
        .finally(() => live && setSearching(false))
    }, 250)
    return () => {
      live = false
      window.clearTimeout(t)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc?.id, doc?.version, searchOpen, query, matchCase])

  useEffect(() => setSelection(null), [doc?.id, doc?.version, editing])

  // Track viewer width for "fit width".
  useEffect(() => {
    const el = viewer.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setViewerWidth(e.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [doc?.id])

  useEffect(() => {
    setCurrent(0)
    setSelected(new Set())
    viewer.current?.scrollTo({ top: 0 })
  }, [activeId])

  const scale = useMemo(() => {
    if (!doc) return 1
    if (!fitWidth) return zoom
    const maxW = Math.max(...doc.info.pages.map((p) => p.width))
    return Math.max(0.2, (viewerWidth - 64) / maxW)
  }, [doc, fitWidth, zoom, viewerWidth])

  // Which page is in view.
  const onScroll = () => {
    const el = viewer.current
    if (!el || !doc) return
    const mid = el.scrollTop + el.clientHeight / 3
    const nodes = el.querySelectorAll<HTMLElement>('[data-page]')
    for (const n of nodes) {
      if (n.offsetTop + n.offsetHeight > mid) {
        setCurrent(Number(n.dataset.page))
        break
      }
    }
  }

  const goTo = (i: number) => {
    const el = viewer.current?.querySelector<HTMLElement>(`[data-page="${i}"]`)
    el?.scrollIntoView({ block: 'start' })
  }

  const showHit = (h: SearchHit) => {
    const el = viewer.current
    const pageEl = el?.querySelector<HTMLElement>(`[data-page="${h.page}"]`)
    if (!el || !pageEl || !doc) return
    const r = h.rects[0]
    const pg = doc.info.pages[h.page]
    const y = r ? (pg.height + pg.y0 - r[3]) * scale : 0
    el.scrollTo({ top: pageEl.offsetTop + y - el.clientHeight / 3 })
  }

  const stepHit = (dir: 1 | -1) => {
    if (!hits.length) return
    const i = (hitIndex + dir + hits.length) % hits.length
    setHitIndex(i)
    showHit(hits[i])
  }

  const copySelection = async () => {
    if (!selection) return
    const text = sliceText(selection.text, selection.sel.start, selection.sel.end)
    try {
      await navigator.clipboard.writeText(text)
    } catch {
      const ta = document.createElement('textarea')
      ta.value = text
      document.body.appendChild(ta)
      ta.select()
      document.execCommand('copy')
      ta.remove()
    }
    notify(`Copié : ${text.length > 60 ? text.slice(0, 60) + '…' : text}`)
  }

  const setZoomStep = (dir: 1 | -1) => {
    const z = fitWidth ? scale : zoom
    const next = dir > 0 ? ZOOMS.find((v) => v > z + 0.01) ?? 4 : [...ZOOMS].reverse().find((v) => v < z - 0.01) ?? 0.25
    setFitWidth(false)
    setZoom(next)
  }

  const replaceDoc = (meta: DocMeta) =>
    setDocs((ds) => ds.map((d) => (d.id === meta.id ? { ...meta, version: d.version + 1 } : d)))

  /** Rebuild the active document from a new page list. */
  const rebuild = async (pages: { page: number; rotate?: number }[], label: string) => {
    if (!doc) return
    const meta = await run(label, () =>
      api.assemblePages(pages.map((p) => ({ doc: doc.id, ...p })), doc.name, doc.id),
    )
    if (meta) replaceDoc(meta)
    return meta
  }

  const targetPages = () => (selected.size ? [...selected].sort((a, b) => a - b) : [current])

  const rotate = async (deg: number) => {
    if (!doc) return
    const t = new Set(targetPages())
    await rebuild(
      doc.info.pages.map((_, i) => ({ page: i, rotate: t.has(i) ? deg : 0 })),
      'Rotation…',
    )
  }

  const deletePages = async () => {
    if (!doc) return
    const t = new Set(targetPages())
    if (t.size >= doc.info.page_count) return notify('Impossible de supprimer toutes les pages.')
    await rebuild(
      doc.info.pages.map((_, i) => ({ page: i })).filter((p) => !t.has(p.page)),
      'Suppression…',
    )
    setSelected(new Set())
  }

  const extractPages = async () => {
    if (!doc) return
    const t = targetPages()
    const base = doc.name.replace(/\.pdf$/i, '')
    const meta = await run('Extraction…', () =>
      api.assemblePages(t.map((page) => ({ doc: doc.id, page })), `${base} (extrait).pdf`),
    )
    if (meta) {
      addDocs([meta])
      notify(`${t.length} page(s) extraite(s) dans un nouveau document.`)
    }
  }

  const movePage = async (from: number, to: number) => {
    if (!doc || from === to) return
    const order = doc.info.pages.map((_, i) => i)
    const [p] = order.splice(from, 1)
    order.splice(to, 0, p)
    await rebuild(order.map((page) => ({ page })), 'Réorganisation…')
    setSelected(new Set([to]))
  }

  const fontsOnline = () => {
    try {
      return localStorage.getItem('kb.fonts.online') !== 'false'
    } catch {
      return true
    }
  }

  const editText = async (page: number, block: TextBlock, text: string, mode: EditMode) => {
    if (!doc) return false
    const res = await run('Modification du texte…', () =>
      api.editBlock(doc.id, page, block, text, mode, fontsOnline()),
    )
    if (!res) return false
    replaceDoc(res.doc)
    const notes = [`Police : ${res.font}`]
    if (res.extra_lines > 0 && mode === 'exact')
      notes.push(
        block.scanned
          ? `le paragraphe a ${res.extra_lines} ligne(s) de plus : raccourcissez-le s'il chevauche la suite`
          : `le paragraphe a ${res.extra_lines} ligne(s) de plus : cochez « Décaler la suite » s'il chevauche le texte en dessous`,
      )
    if (!res.lossless && mode === 'exact') notes.push('page réécrite (structure inhabituelle)')
    notify(notes.join(' · '))
    return true
  }

  selectionRef.current = selection

  const addAnnot = async (page: number, a: NewAnnot) => {
    if (!doc) return
    const meta = await run('Annotation…', () => api.addAnnot(doc.id, page, a))
    if (meta) replaceDoc(meta)
  }

  // Form fields of the active document.
  useEffect(() => {
    if (!doc) return setFields([])
    let live = true
    api
      .listFields(doc.id)
      .then((f) => {
        if (!live) return
        setFields(f)
        if (f.some((x) => !x.readonly && x.kind !== 'button') && !formNoticed.current.has(doc.id)) {
          formNoticed.current.add(doc.id)
          notify('Ce document contient un formulaire : cliquez dans les champs pour le remplir.')
        }
      })
      .catch(() => live && setFields([]))
    return () => {
      live = false
    }
  }, [doc?.id, doc?.version, notify])

  // Fills run one after another: leaving a text field by clicking a checkbox
  // fires two changes at once, and each must see the previous result.
  const fillQueue = useRef<Promise<unknown>>(Promise.resolve())
  const fillField = (name: string, value: string) => {
    if (!doc) return
    const id = doc.id
    fillQueue.current = fillQueue.current.then(async () => {
      const meta = await run('Formulaire…', () => api.fillFields(id, [{ name, value }]))
      if (meta) replaceDoc(meta)
    })
  }

  const flattenDoc = async () => {
    if (!doc) return
    const meta = await run('Aplatissement…', () => api.flattenDoc(doc.id))
    if (meta) {
      replaceDoc(meta)
      notify('Formulaire et annotations intégrés à la page. Pensez à enregistrer.')
    }
  }

  const pickedSignature = (image: PlacedImage) => {
    const target = signOpen
    setSignOpen(null)
    if (target && target !== true) {
      // Signature field: fit the signature inside it.
      const [x0, y0, x1, y1] = target.rect
      const ratio = image.height / image.width
      const w = Math.min(x1 - x0, (y1 - y0) / ratio)
      const h = w * ratio
      const cx = (x0 + x1) / 2
      const cy = (y0 + y1) / 2
      addAnnot(target.page, { kind: 'image', rect: [cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2], data: image.data })
      return
    }
    setEditing(false)
    setAnnotating(false)
    setPlacing({ image, width: 150, what: 'la signature' })
  }

  const placeImage = async (page: number, rect: Rect) => {
    if (!placing) return
    const data = placing.image.data
    setPlacing(null)
    await addAnnot(page, { kind: 'image', rect, data })
    notify('Ajouté. Pour le retirer : Annoter → ↖ puis cliquez dessus.')
  }

  const addText = async (page: number, x: number, y: number, text: string) => {
    if (!doc) return
    const meta = await run('Ajout du texte…', () => api.addText(doc.id, page, x, y, text, textStyle))
    if (meta) replaceDoc(meta)
  }

  const applyWatermark = async (w: Watermark) => {
    if (!doc) return
    setDecorate(null)
    const meta = await run('Filigrane…', () => api.addWatermark(doc.id, w))
    if (meta) {
      replaceDoc(meta)
      notify('Filigrane ajouté. Ctrl/⌘+Z pour annuler.')
    }
  }

  const applyHeaderFooter = async (h: HeaderFooter) => {
    if (!doc) return
    setDecorate(null)
    const meta = await run('Mise en page…', () => api.addHeaderFooter(doc.id, h))
    if (meta) {
      replaceDoc(meta)
      notify('Ajouté sur les pages. Ctrl/⌘+Z pour annuler.')
    }
  }

  const openLocked = async (password: string) => {
    const path = locked[0]
    try {
      const meta = await api.openLocked(path, password)
      setLockError(null)
      setLocked((l) => l.slice(1))
      addDocs([meta])
    } catch (e) {
      setLockError(String(e).includes('incorrect') ? 'Mot de passe incorrect.' : String(e))
    }
  }

  const applyProtection = async (p: ProtectOptions | null) => {
    if (!doc) return
    setDocDialog(null)
    const meta = await run('Protection…', () => api.setProtection(doc.id, p))
    if (meta) {
      replaceDoc(meta)
      notify(p ? 'La protection sera appliquée à l\'enregistrement.' : 'Protection retirée : enregistrez pour l\'appliquer.')
    }
  }

  const applyCompress = async (level: CompressLevel) => {
    if (!doc) return
    setDocDialog(null)
    const res = await run('Compression…', () => api.compressDoc(doc.id, level))
    if (!res) return
    replaceDoc(res.doc)
    const { before, after, images } = res.result
    const mb = (n: number) => (n / 1048576).toFixed(n < 1048576 ? 2 : 1).replace('.', ',') + ' Mo'
    notify(
      after < before
        ? `${mb(before)} → ${mb(after)} (−${Math.round((1 - after / before) * 100)} %${images ? `, ${images} image(s)` : ''}). Pensez à enregistrer.`
        : 'Ce document est déjà compact : rien à gagner à ce niveau.',
    )
  }

  const applyCrop = async (pages: number[] | null, crop: Crop) => {
    if (!doc) return
    setDocDialog(null)
    const meta = await run('Rognage…', () => api.cropDoc(doc.id, pages, crop))
    if (meta) replaceDoc(meta)
  }

  const applySplit = async (groups: number[][]) => {
    if (!doc) return
    const dir = await pickFolder()
    if (!dir) return
    setDocDialog(null)
    const base = doc.name.replace(/\.pdf$/i, '')
    const files = await run('Division…', () => api.splitDoc(doc.id, groups, dir, base))
    if (files) notify(`${files.length} fichier(s) créé(s) dans ${dir}`)
  }

  const applyConvert = async (target: ConvertTarget, pages: number[], withOcr: boolean) => {
    if (!doc) return
    const base = doc.name.replace(/\.pdf$/i, '')
    const dest =
      target.kind === 'images'
        ? await pickFolder()
        : target.kind === 'word'
          ? await pickSaveAs(`${base}.docx`, 'Document Word', ['docx'])
          : target.kind === 'excel'
            ? await pickSaveAs(`${base}.xlsx`, 'Classeur Excel', ['xlsx'])
            : await pickSaveAs(`${base}.txt`, 'Texte', ['txt'])
    if (!dest) return
    setConvert(null)
    const files = await run('Conversion…', () => api.convertDoc(doc.id, target, pages, dest, withOcr, []))
    if (!files?.length) return
    notify(files.length > 1 ? `${files.length} images créées` : `Converti : ${files[0].split(/[\\/]/).pop()}`)
    revealFile(files[0]).catch(() => {})
  }

  const saveBookmarks = async (list: Bookmark[]) => {
    if (!doc) return
    const meta = await run('Signets…', () => api.setBookmarks(doc.id, list))
    if (meta) replaceDoc(meta)
  }

  const startPrint = (pages: number[], dpi: number) => {
    if (!doc) return
    setPrintOpen(false)
    setPrintJob({ pages: pages.map((i) => ({ index: i, ...doc.info.pages[i] })), dpi })
  }

  // Highlight / underline / strike out the text just selected.
  const markSelection = async () => {
    const s = selectionRef.current
    if (!annotating || !MARKUP_TOOLS.includes(tool) || !s) return
    const { sel, text } = s
    const rects = lineRects(text.boxes.slice(Math.min(sel.start, sel.end), Math.max(sel.start, sel.end) + 1))
    setSelection(null)
    if (rects.length) await addAnnot(sel.page, { kind: tool as 'highlight', rects, color: annotColor })
  }

  const runOcr = async (languages: string[], pages: number[] | null) => {
    if (!doc) return
    setOcrOpen(false)
    const res = await run('Reconnaissance du texte…', () => api.ocrDoc(doc.id, languages, pages))
    if (!res) return
    if (res.doc) replaceDoc(res.doc)
    const s = res.summary
    if (s.pages.length)
      notify(
        `Texte reconnu sur ${s.pages.length} page(s) : ${s.words} mots, fiabilité ${Math.round(s.confidence)} %. ` +
          'Il peut maintenant être cherché, copié et modifié (« Modifier le texte »).',
      )
    else if (s.skipped.length) notify('Ces pages contiennent déjà du texte : rien à reconnaître.')
    else notify('Aucun texte trouvé sur ces pages.')
  }

  const fromImages = async (paths: string[] | null, format: PageFormat, margin: number) => {
    setNewOpen(false)
    const list = paths ?? (await pickImagePaths())
    if (!list.length) return
    const meta = await run('Création du PDF…', () => api.createFromImages(list, format, margin))
    if (meta) {
      addDocs([meta])
      notify(`PDF créé : ${meta.info.page_count} page(s). Pour un scan, utilisez « Reconnaître le texte ».`)
    }
  }
  const fromImagesRef = useRef(fromImages)
  fromImagesRef.current = fromImages

  const createBlank = async (count: number, format: PageFormat, landscape: boolean) => {
    setNewOpen(false)
    const meta = await run('Création…', () => api.createBlank(count, format, landscape))
    if (meta) addDocs([meta])
  }

  const insertBlank = async () => {
    if (!doc) return
    const meta = await run('Ajout d’une page…', () => api.insertBlank(doc.id, current + 1))
    if (meta) {
      replaceDoc(meta)
      notify(`Page vierge ajoutée après la page ${current + 1}.`)
    }
  }

  const undo = async () => {
    if (!doc) return
    const meta = await run('Annulation…', () => api.undo(doc.id))
    if (meta) replaceDoc(meta)
    else if (meta === null) notify('Rien à annuler.')
  }

  const save = async (as = false) => {
    if (!doc) return
    let path = doc.path
    if (as || !path) path = await pickSavePath(doc.name)
    if (!path) return
    const name = await run('Enregistrement…', () => api.saveDoc(doc.id, path!))
    if (name) {
      setDocs((ds) => ds.map((d) => (d.id === doc.id ? { ...d, name, path, dirty: false } : d)))
      notify(`Enregistré : ${name}`)
    }
  }

  const close = async (id: string) => {
    const d = docs.find((x) => x.id === id)
    if (d?.dirty && !window.confirm(`« ${d.name} » a des modifications non enregistrées. Fermer quand même ?`)) return
    await api.closeDoc(id)
    const rest = docs.filter((x) => x.id !== id)
    setDocs(rest)
    if (activeId === id) setActiveId(rest.length ? rest[rest.length - 1].id : null)
  }

  const merge = async (ids: string[], name: string) => {
    setMerging(false)
    const meta = await run('Fusion…', () => api.mergeDocs(ids, name))
    if (meta) {
      addDocs([meta])
      notify(`Fusion terminée : ${meta.info.page_count} pages. Pensez à enregistrer.`)
    }
  }

  // Keyboard shortcuts.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey
      const typing = e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement
      if (mod && e.key === 'f' && doc) return (e.preventDefault(), setSearchOpen(true))
      if (e.key === 'F3' && searchOpen) return (e.preventDefault(), stepHit(e.shiftKey ? -1 : 1))
      if (mod && e.key === 'c' && !typing && selection) return (e.preventDefault(), copySelection())
      if (e.key === 'Escape' && selection) setSelection(null)
      if (e.key === 'Escape' && placing) setPlacing(null)
      if (e.key === 'Escape' && addingText && !typing) setAddingText(false)
      if (mod && e.key === 'o') (e.preventDefault(), openDialog())
      else if (mod && e.key === 's') (e.preventDefault(), save(e.shiftKey))
      else if (mod && e.key === 'p' && doc) (e.preventDefault(), setPrintOpen(true))
      else if (mod && (e.key === '+' || e.key === '=')) (e.preventDefault(), setZoomStep(1))
      else if (mod && e.key === '-') (e.preventDefault(), setZoomStep(-1))
      else if (mod && e.key === '0') (e.preventDefault(), setFitWidth(true))
      else if (mod && e.key === 'z' && !(e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLInputElement))
        (e.preventDefault(), undo())
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const dragFrom = useRef<number | null>(null)

  return (
    <div className="app">
      <header className="toolbar">
        <div className="brand">KB PDF</div>
        <Menu
          label="Fichier"
          items={[
            { label: 'Nouveau…', hint: 'images, scans, vierge', onClick: () => setNewOpen(true) },
            { label: 'Ouvrir…', hint: 'Ctrl/⌘+O', onClick: openDialog },
            '—',
            { label: 'Enregistrer', hint: 'Ctrl/⌘+S', onClick: () => save(false), disabled: !doc },
            { label: 'Enregistrer sous…', hint: 'Maj+Ctrl/⌘+S', onClick: () => save(true), disabled: !doc },
            { label: 'Imprimer…', hint: 'Ctrl/⌘+P', onClick: () => setPrintOpen(true), disabled: !doc },
            '—',
            { label: 'Fusionner des PDF…', onClick: () => setMerging(true) },
            { label: 'Traitement par lots…', hint: 'plusieurs fichiers', onClick: () => setBatchOpen(true) },
            { label: 'Comparer deux PDF…', onClick: () => setComparing(true), disabled: !doc },
            { label: 'Compresser…', onClick: () => setDocDialog('compress'), disabled: !doc },
            { label: 'Protéger par mot de passe…', onClick: () => setDocDialog('protect'), disabled: !doc },
            '—',
            { label: 'Licence et à propos…', onClick: () => setLicence('about') },
          ]}
        />
        <Menu
          label="Pages"
          disabled={!doc}
          items={[
            { label: 'Pivoter à gauche', onClick: () => rotate(270) },
            { label: 'Pivoter à droite', onClick: () => rotate(90) },
            '—',
            { label: 'Insérer une page vierge', hint: 'après la page actuelle', onClick: insertBlank },
            { label: 'Extraire dans un nouveau PDF', onClick: extractPages },
            { label: 'Diviser le document…', onClick: () => setDocDialog('split') },
            { label: 'Rogner…', onClick: () => setDocDialog('crop') },
            { label: 'Supprimer', onClick: deletePages, danger: true },
          ]}
        />
        <Menu
          label="Convertir"
          disabled={!doc}
          items={[
            { label: 'En Word (.docx)…', onClick: () => setConvert('word') },
            { label: 'En Excel (.xlsx)…', onClick: () => setConvert('excel') },
            { label: 'En texte (.txt)…', onClick: () => setConvert('text') },
            { label: 'En images (PNG, JPEG)…', onClick: () => setConvert('images') },
            '—',
            { label: 'Images en PDF…', hint: 'nouveau document', onClick: () => setNewOpen(true) },
          ]}
        />
        <Menu
          label="Outils"
          disabled={!doc}
          items={[
            { label: 'Reconnaître le texte (OCR)…', onClick: () => setOcrOpen(true), disabled: !ocr },
            { label: 'Signer…', onClick: () => setSignOpen(true) },
            '—',
            {
              label: 'Ajouter du texte',
              hint: 'cliquez sur la page',
              onClick: () => {
                setEditing(false)
                setAnnotating(false)
                setPlacing(null)
                setAddingText(true)
              },
            },
            { label: 'Ajouter une image…', onClick: () => imageInput.current?.click() },
            { label: 'Filigrane…', onClick: () => setDecorate('watermark') },
            { label: 'Numéros de page…', onClick: () => setDecorate('numbers') },
            { label: 'En-tête et pied de page…', onClick: () => setDecorate('header') },
            '—',
            {
              label: 'Aplatir formulaire et annotations',
              hint: 'les rend non modifiables',
              onClick: flattenDoc,
            },
          ]}
        />
        <span className="sep" />
        <button onClick={openDialog} title="Ouvrir (Ctrl/⌘+O)">Ouvrir</button>
        <button onClick={() => save(false)} disabled={!doc} title="Enregistrer (Ctrl/⌘+S)">Enregistrer</button>
        <span className="sep" />
        <button
          className={editing ? 'on' : ''}
          onClick={() => {
            setEditing((v) => !v)
            setAnnotating(false)
            setPlacing(null)
            setAddingText(false)
          }}
          disabled={!doc}
          title="Cliquer sur un paragraphe pour le modifier"
        >
          Modifier le texte
        </button>
        <button
          className={annotating ? 'on' : ''}
          onClick={() => {
            setAnnotating((v) => !v)
            setEditing(false)
            setPlacing(null)
            setAddingText(false)
          }}
          disabled={!doc}
          title="Surligner, commenter, dessiner, tamponner"
        >
          Annoter
        </button>
        <button onClick={() => setSignOpen(true)} disabled={!doc} title="Signer le document">Signer</button>
        <button onClick={() => setSearchOpen(true)} disabled={!doc} title="Rechercher (Ctrl/⌘+F)">Rechercher</button>
        <button onClick={undo} disabled={!doc} title="Annuler (Ctrl/⌘+Z)">↶</button>
        <span className="grow" />
        {doc && (
          <div className="pager">
            <input
              aria-label="Page"
              value={current + 1}
              onChange={(e) => {
                const n = Number(e.target.value) - 1
                if (n >= 0 && n < doc.info.page_count) goTo(n)
              }}
            />
            <span className="muted">/ {doc.info.page_count}</span>
          </div>
        )}
        <button onClick={() => setZoomStep(-1)} disabled={!doc} aria-label="Dézoomer">−</button>
        <button className="zoom" onClick={() => setFitWidth(true)} disabled={!doc} title="Ajuster à la largeur">
          {Math.round(scale * 100)} %
        </button>
        <button onClick={() => setZoomStep(1)} disabled={!doc} aria-label="Zoomer">+</button>
      </header>

      {docs.length > 0 && (
        <nav className="tabs">
          {docs.map((d) => (
            <div key={d.id} className={`tab ${d.id === activeId ? 'active' : ''}`} onClick={() => setActiveId(d.id)}>
              <span title={d.path ?? d.name}>{d.dirty ? '• ' : ''}{d.name}</span>
              <button className="icon" onClick={(e) => (e.stopPropagation(), close(d.id))} aria-label="Fermer">✕</button>
            </div>
          ))}
        </nav>
      )}

      <main className="body">
        {doc ? (
          <>
            <aside className="sidebar">
              <div className="side-tabs">
                <button className={side === 'pages' ? 'on' : ''} onClick={() => setSide('pages')}>Pages</button>
                <button className={side === 'bookmarks' ? 'on' : ''} onClick={() => setSide('bookmarks')}>Signets</button>
                <button className={side === 'fonts' ? 'on' : ''} onClick={() => setSide('fonts')}>Polices</button>
              </div>
              {side === 'pages' ? (
                <div className="thumbs">
                  {doc.info.pages.map((p, i) => (
                    <div
                      key={i}
                      className={`thumb ${i === current ? 'current' : ''} ${selected.has(i) ? 'selected' : ''}`}
                      draggable
                      onDragStart={() => (dragFrom.current = i)}
                      onDragOver={(e) => e.preventDefault()}
                      onDrop={() => dragFrom.current !== null && movePage(dragFrom.current, i)}
                      onClick={(e) => {
                        if (e.metaKey || e.ctrlKey) {
                          const s = new Set(selected)
                          s.has(i) ? s.delete(i) : s.add(i)
                          setSelected(s)
                        } else if (e.shiftKey && selected.size) {
                          const a = Math.min(...selected, i), b = Math.max(...selected, i)
                          setSelected(new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k)))
                        } else {
                          setSelected(new Set([i]))
                          goTo(i)
                        }
                      }}
                    >
                      <PageImage
                        docId={doc.id}
                        version={doc.version}
                        page={i}
                        width={THUMB_W}
                        height={(THUMB_W * p.height) / p.width}
                      />
                      <span className="thumb-num">{i + 1}</span>
                    </div>
                  ))}
                </div>
              ) : side === 'bookmarks' ? (
                <BookmarksPanel docId={doc.id} version={doc.version} current={current} onGo={goTo} onSave={saveBookmarks} />
              ) : (
                <FontsPanel docId={doc.id} version={doc.version} />
              )}
            </aside>
            <div className="viewer-col">
            {ocr && ocr.key.startsWith(`${doc.id}:`) && ocr.scanned.length > 0 && !bannerHidden.has(doc.id) && (
              <div className="scan-banner">
                <span>
                  {ocr.scanned.length === doc.info.page_count
                    ? 'Ce document semble scanné : son texte ne peut pas encore être cherché ni modifié.'
                    : `${ocr.scanned.length} page(s) semblent scannées.`}
                </span>
                <button className="primary" onClick={() => setOcrOpen(true)}>Reconnaître le texte</button>
                <button className="icon" aria-label="Masquer" onClick={() => setBannerHidden(new Set(bannerHidden).add(doc.id))}>✕</button>
              </div>
            )}
            {addingText && (
              <div className="scan-banner">
                <span>Cliquez sur la page à l'endroit où écrire. Le texte fera partie de la page.</span>
                <button className="ghost" onClick={() => setAddingText(false)}>Terminer</button>
              </div>
            )}
            {placing && (
              <div className="scan-banner">
                <span>Cliquez sur la page pour placer {placing.what}, ou tracez un cadre. Échap pour annuler.</span>
                <button className="ghost" onClick={() => setPlacing(null)}>Annuler</button>
              </div>
            )}
            {annotating && (
              <AnnotToolbar
                tool={tool}
                color={annotColor}
                width={annotWidth}
                stamp={stamp}
                onTool={(t) => {
                  setTool(t)
                  // Sensible default colours per tool.
                  if (t === 'highlight') setAnnotColor([255, 230, 0])
                  else if (t === 'stamp' || t === 'strike_out') setAnnotColor([220, 30, 30])
                  else if (t !== 'note' && t !== 'select' && 0.299 * annotColor[0] + 0.587 * annotColor[1] + 0.114 * annotColor[2] > 160)
                    setAnnotColor([30, 80, 220]) // light colours are hard to see as lines
                }}
                onColor={setAnnotColor}
                onWidth={setAnnotWidth}
                onStamp={setStamp}
                onClose={() => setAnnotating(false)}
              />
            )}
            {searchOpen && (
              <SearchBar
                query={query}
                matchCase={matchCase}
                count={hits.length}
                index={hitIndex}
                searching={searching}
                onQuery={setQuery}
                onMatchCase={setMatchCase}
                onStep={stepHit}
                onClose={() => {
                  setSearchOpen(false)
                  setHits([])
                }}
              />
            )}
            <section className={`viewer ${editing ? 'editing' : ''}`} ref={viewer} onScroll={onScroll}>
              {doc.info.pages.map((p, i) => (
                <div key={i} data-page={i} className="page-wrap">
                  <div className="page-box" style={{ width: p.width * scale, height: p.height * scale }}>
                    <PageImage
                      docId={doc.id}
                      version={doc.version}
                      page={i}
                      width={p.width * scale}
                      height={p.height * scale}
                    />
                    {/* Overlays work in page coordinates: shift them when the visible area does not start at 0, 0. */}
                    <div
                      className="page-origin"
                      style={{ left: -p.x0 * scale, width: (p.width + p.x0) * scale, height: (p.height + p.y0) * scale }}
                    >
                      {!editing && (Math.abs(i - current) <= 2 || hits.some((h) => h.page === i)) && (
                        <TextLayer
                          docId={doc.id}
                          version={doc.version}
                          page={i}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          hits={hits.map((hit, n) => ({ hit, active: n === hitIndex })).filter((h) => h.hit.page === i)}
                          selection={selection?.sel ?? null}
                          onSelect={(sel, text) => setSelection(sel && text ? { sel, text } : null)}
                          onCopy={copySelection}
                          onSelectEnd={markSelection}
                          noChip={annotating && MARKUP_TOOLS.includes(tool)}
                        />
                      )}
                      {annotating && Math.abs(i - current) <= 2 && (
                        <AnnotLayer
                          docId={doc.id}
                          version={doc.version}
                          page={i}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          tool={tool}
                          color={annotColor}
                          width={annotWidth}
                          stamp={stamp}
                          onAdd={addAnnot}
                          onDelete={async (pg, index) => {
                            const meta = await run('Suppression…', () => api.deleteAnnot(doc.id, pg, index))
                            if (meta) replaceDoc(meta)
                          }}
                          onNote={async (pg, index, text) => {
                            const meta = await run('Enregistrement…', () => api.setNote(doc.id, pg, index, text))
                            if (meta) replaceDoc(meta)
                          }}
                        />
                      )}
                      {!editing && !annotating && !placing && !addingText && Math.abs(i - current) <= 2 && fields.some((f) => f.page === i) && (
                        <FormLayer
                          fields={fields.filter((f) => f.page === i)}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          onFill={fillField}
                          onSign={(rect) => setSignOpen({ page: i, rect })}
                        />
                      )}
                      {addingText && Math.abs(i - current) <= 2 && (
                        <TextPlaceLayer
                          page={i}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          style={textStyle}
                          onStyle={setTextStyle}
                          origin={[p.x0, p.y0]}
                          onAdd={addText}
                        />
                      )}
                      {placing && Math.abs(i - current) <= 2 && (
                        <PlaceLayer
                          image={placing.image}
                          page={i}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          width={placing.width}
                          onPlace={placeImage}
                        />
                      )}
                      {editing && Math.abs(i - current) <= 2 && (
                        <EditLayer
                          docId={doc.id}
                          version={doc.version}
                          page={i}
                          pageHeight={p.height + p.y0}
                          scale={scale}
                          onEdit={editText}
                        />
                      )}
                    </div>
                  </div>
                </div>
              ))}
            </section>
            </div>
          </>
        ) : (
          <div className="welcome">
            <h1>KB PDF Reader</h1>
            <p className="muted">Glissez des PDF ici, ou</p>
            <div className="welcome-actions">
              <button className="primary" onClick={openDialog}>Ouvrir un PDF</button>
              <button onClick={() => setNewOpen(true)}>Créer un PDF</button>
              <button onClick={() => setMerging(true)}>Fusionner des PDF</button>
            </div>
          </div>
        )}
      </main>

      {comparing && doc && (
        <CompareView
          docs={docs}
          initial={doc.id}
          onOpenFiles={async () => {
            const paths = await pickPdfPaths()
            return paths.length ? openPaths(paths, false) : []
          }}
          onClose={() => setComparing(false)}
        />
      )}
      {batchOpen && (
        <BatchDialog
          initial={docs.map((d) => d.path).filter((p): p is string => !!p)}
          ocrAvailable={ocr ? ocr.languages.length > 0 : true}
          onClose={() => setBatchOpen(false)}
        />
      )}
      {merging && (
        <MergeDialog
          docs={docs}
          onAddFiles={async () => {
            const paths = await pickPdfPaths()
            return paths.length ? openPaths(paths, false) : []
          }}
          onMerge={merge}
          onClose={() => setMerging(false)}
        />
      )}
      {ocrOpen && doc && ocr && (
        <OcrDialog
          languages={ocr.languages}
          scanned={ocr.scanned}
          current={current}
          pageCount={doc.info.page_count}
          onRun={runOcr}
          onClose={() => setOcrOpen(false)}
        />
      )}
      {newOpen && (
        <NewDialog
          onBlank={createBlank}
          onImages={(format, margin) => fromImages(null, format, margin)}
          onClose={() => setNewOpen(false)}
        />
      )}
      {decorate === 'watermark' && doc && (
        <WatermarkDialog pageCount={doc.info.page_count} onApply={applyWatermark} onClose={() => setDecorate(null)} />
      )}
      {(decorate === 'header' || decorate === 'numbers') && doc && (
        <HeaderFooterDialog
          pageCount={doc.info.page_count}
          fileName={doc.name}
          numbersOnly={decorate === 'numbers'}
          onApply={applyHeaderFooter}
          onClose={() => setDecorate(null)}
        />
      )}
      {locked.length > 0 && (
        <PasswordDialog
          key={locked[0]}
          name={locked[0].split(/[\\/]/).pop() ?? locked[0]}
          error={lockError}
          onOpen={openLocked}
          onSkip={() => {
            setLockError(null)
            setLocked((l) => l.slice(1))
          }}
        />
      )}
      {docDialog === 'protect' && doc && <ProtectDialog docId={doc.id} onApply={applyProtection} onClose={() => setDocDialog(null)} />}
      {docDialog === 'compress' && doc && <CompressDialog onApply={applyCompress} onClose={() => setDocDialog(null)} />}
      {docDialog === 'crop' && doc && (
        <CropDialog pageCount={doc.info.page_count} current={current} onApply={applyCrop} onClose={() => setDocDialog(null)} />
      )}
      {docDialog === 'split' && doc && (
        <SplitDialog pageCount={doc.info.page_count} onApply={applySplit} onClose={() => setDocDialog(null)} />
      )}
      {convert && doc && (
        <ConvertDialog
          kind={convert}
          pageCount={doc.info.page_count}
          current={current}
          scanned={ocr?.key.startsWith(`${doc.id}:`) ? ocr.scanned : []}
          ocrAvailable={!!ocr?.languages.length}
          onKind={setConvert}
          onApply={applyConvert}
          onClose={() => setConvert(null)}
        />
      )}
      {printOpen && doc && (
        <PrintDialog pageCount={doc.info.page_count} current={current} onPrint={startPrint} onClose={() => setPrintOpen(false)} />
      )}
      {printJob && doc && (
        <PrintRoot
          docId={doc.id}
          pages={printJob.pages}
          dpi={printJob.dpi}
          onProgress={(d, t) => setBusy(d < t ? `Préparation de l'impression… ${d}/${t}` : null)}
          onDone={(e) => {
            setBusy(null)
            setPrintJob(null)
            if (e) notify(`Impression impossible : ${e}`)
          }}
        />
      )}
      {signOpen && <SignatureDialog onClose={() => setSignOpen(null)} onPick={pickedSignature} />}
      <input
        ref={imageInput}
        type="file"
        hidden
        accept="image/png,image/jpeg,image/webp,image/bmp"
        onChange={async (e) => {
          const file = e.target.files?.[0]
          e.target.value = ''
          if (!file) return
          const image = await fromFile(file, false)
          setEditing(false)
          setAnnotating(false)
          // Default: a third of the page width.
          setPlacing({ image, width: doc ? doc.info.pages[current].width / 3 : 200, what: "l'image" })
        }}
      />
      {licence && (
        <LicenseDialog mustAccept={licence === 'first'} onAccept={() => setLicence(null)} onClose={() => setLicence(null)} />
      )}
      {dragOver && <div className="drop-hint">Déposez des PDF pour les ouvrir, ou des images pour en faire un PDF</div>}
      {busy && <div className="busy">{busy}</div>}
      {toast && <div className="toast" onClick={() => setToast(null)}>{toast}</div>}
    </div>
  )
}
