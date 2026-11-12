import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api, pickPdfPaths, pickSavePath, type DocMeta } from './lib/api'
import { PageImage } from './components/PageImage'
import { MergeDialog } from './components/MergeDialog'
import { FontsPanel } from './components/FontsPanel'
import { EditLayer } from './components/EditLayer'
import type { EditMode, TextBlock } from './lib/api'

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
  const [side, setSide] = useState<'pages' | 'fonts'>('pages')
  const [merging, setMerging] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [editing, setEditing] = useState(false)
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
            if (pdfs.length) openPaths(pdfs)
          }
        })
        .then((u) => (unlisten = u)),
    )
    return () => unlisten?.()
  }, [openPaths])

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
      notes.push(`le paragraphe a ${res.extra_lines} ligne(s) de plus : cochez « Décaler la suite » s'il chevauche le texte en dessous`)
    if (!res.lossless && mode === 'exact') notes.push('page réécrite (structure inhabituelle)')
    notify(notes.join(' · '))
    return true
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
      if (mod && e.key === 'o') (e.preventDefault(), openDialog())
      else if (mod && e.key === 's') (e.preventDefault(), save(e.shiftKey))
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
        <button onClick={openDialog} title="Ouvrir (Ctrl/⌘+O)">Ouvrir</button>
        <button onClick={() => save(false)} disabled={!doc} title="Enregistrer (Ctrl/⌘+S)">Enregistrer</button>
        <button onClick={() => save(true)} disabled={!doc}>Enregistrer sous…</button>
        <span className="sep" />
        <button onClick={() => setMerging(true)} title="Fusionner plusieurs PDF">Fusionner</button>
        <span className="sep" />
        <button
          className={editing ? 'on' : ''}
          onClick={() => setEditing((v) => !v)}
          disabled={!doc}
          title="Cliquer sur un paragraphe pour le modifier"
        >
          Modifier le texte
        </button>
        <button onClick={undo} disabled={!doc} title="Annuler (Ctrl/⌘+Z)">↶</button>
        <span className="sep" />
        <button onClick={() => rotate(270)} disabled={!doc} title="Pivoter à gauche">⟲</button>
        <button onClick={() => rotate(90)} disabled={!doc} title="Pivoter à droite">⟳</button>
        <button onClick={extractPages} disabled={!doc}>Extraire</button>
        <button onClick={deletePages} disabled={!doc} className="danger">Supprimer</button>
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
              ) : (
                <FontsPanel docId={doc.id} version={doc.version} />
              )}
            </aside>
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
                    {editing && Math.abs(i - current) <= 2 && (
                      <EditLayer
                        docId={doc.id}
                        version={doc.version}
                        page={i}
                        pageHeight={p.height}
                        scale={scale}
                        onEdit={editText}
                      />
                    )}
                  </div>
                </div>
              ))}
            </section>
          </>
        ) : (
          <div className="welcome">
            <h1>KB PDF Reader</h1>
            <p className="muted">Glissez des PDF ici, ou</p>
            <div className="welcome-actions">
              <button className="primary" onClick={openDialog}>Ouvrir un PDF</button>
              <button onClick={() => setMerging(true)}>Fusionner des PDF</button>
            </div>
          </div>
        )}
      </main>

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
      {dragOver && <div className="drop-hint">Déposez vos PDF pour les ouvrir</div>}
      {busy && <div className="busy">{busy}</div>}
      {toast && <div className="toast" onClick={() => setToast(null)}>{toast}</div>}
    </div>
  )
}
