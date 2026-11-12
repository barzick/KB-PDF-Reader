import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { api, parsePages } from '../lib/api'

export function PrintDialog({
  pageCount,
  current,
  onPrint,
  onClose,
}: {
  pageCount: number
  current: number
  onPrint: (pages: number[], dpi: number) => void
  onClose: () => void
}) {
  const [which, setWhich] = useState<'all' | 'current' | 'range'>('all')
  const [range, setRange] = useState('')
  const [dpi, setDpi] = useState(200)
  let pages: number[] = []
  let error = ''
  try {
    pages =
      which === 'all'
        ? Array.from({ length: pageCount }, (_, i) => i)
        : which === 'current'
          ? [current]
          : (parsePages(range, pageCount) ?? [])
  } catch (e) {
    error = (e as Error).message
  }
  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Imprimer" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Imprimer</h2>
        <label className="choice">
          <input type="radio" checked={which === 'all'} onChange={() => setWhich('all')} /> Toutes les pages ({pageCount})
        </label>
        <label className="choice">
          <input type="radio" checked={which === 'current'} onChange={() => setWhich('current')} /> Page actuelle ({current + 1})
        </label>
        <label className="choice">
          <input type="radio" checked={which === 'range'} onChange={() => setWhich('range')} />
          <span>
            Pages :{' '}
            <input
              value={range}
              placeholder="1-3, 5"
              onFocus={() => setWhich('range')}
              onChange={(e) => setRange(e.target.value)}
            />
          </span>
        </label>
        {error && <span className="small danger-text">{error}</span>}
        <label className="field">
          Qualité
          <select value={dpi} onChange={(e) => setDpi(Number(e.target.value))}>
            <option value={150}>Brouillon (150 ppp)</option>
            <option value={200}>Normale (200 ppp)</option>
            <option value={300}>Haute (300 ppp)</option>
          </select>
        </label>
        <p className="small muted">La fenêtre d'impression du système s'ouvrira : choisissez-y l'imprimante, le recto verso et le nombre de copies.</p>
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={!pages.length || !!error} onClick={() => onPrint(pages, dpi)}>
            Imprimer…
          </button>
        </div>
      </div>
    </div>
  )
}

/** Off-screen page images; opens the system print dialog once they are ready. */
export function PrintRoot({
  docId,
  pages,
  dpi,
  onProgress,
  onDone,
}: {
  docId: string
  pages: { index: number; width: number; height: number }[]
  dpi: number
  onProgress: (done: number, total: number) => void
  onDone: (error?: string) => void
}) {
  const [urls, setUrls] = useState<string[]>([])
  const started = useRef(false)

  useEffect(() => {
    let live = true
    const made: string[] = []
    ;(async () => {
      try {
        for (const [n, p] of pages.entries()) {
          const px = Math.round((p.width / 72) * dpi)
          const buf = await api.renderPage(docId, p.index, px)
          if (!live) return
          made.push(URL.createObjectURL(new Blob([buf], { type: 'image/png' })))
          setUrls([...made])
          onProgress(n + 1, pages.length)
        }
      } catch (e) {
        onDone(String(e))
      }
    })()
    return () => {
      live = false
      made.forEach((u) => URL.revokeObjectURL(u))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (started.current || urls.length !== pages.length || !pages.length) return
    started.current = true
    const done = () => onDone()
    window.addEventListener('afterprint', done, { once: true })
    // Let the images lay out before printing.
    requestAnimationFrame(() =>
      setTimeout(() => {
        api.printView().catch((e) => onDone(String(e)))
      }, 150),
    )
  }, [urls, pages.length, onDone])

  return createPortal(
    <div id="print-root">
      <style>{`@page { margin: 0; }`}</style>
      {urls.map((u, i) => (
        <div
          key={u}
          className="print-page"
          style={{ aspectRatio: `${pages[i].width} / ${pages[i].height}` }}
        >
          <img src={u} alt="" />
        </div>
      ))}
    </div>,
    document.body,
  )
}
