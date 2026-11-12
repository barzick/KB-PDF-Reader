import { useState } from 'react'
import { parsePages, type ConvertTarget } from '../lib/api'

export type ConvertKind = ConvertTarget['kind']

const TITLES: Record<ConvertKind, string> = {
  word: 'Convertir en Word',
  excel: 'Convertir en Excel',
  text: 'Convertir en texte',
  images: 'Convertir en images',
}

const NOTES: Record<ConvertKind, string> = {
  word: 'Le texte, les polices, le gras, l’italique, les images et les tableaux sont repris ; une page du PDF devient une section du document Word.',
  excel: 'Une feuille par page. Le texte aligné en colonnes est réparti dans des cellules et les nombres deviennent de vrais nombres.',
  text: 'Le texte brut, dans l’ordre de lecture, sans mise en forme.',
  images: 'Une image par page, enregistrée dans le dossier de votre choix.',
}

export function ConvertDialog({
  kind,
  pageCount,
  current,
  scanned,
  ocrAvailable,
  onKind,
  onApply,
  onClose,
}: {
  kind: ConvertKind
  pageCount: number
  current: number
  /** Pages (1-based) that look scanned. */
  scanned: number[]
  ocrAvailable: boolean
  onKind: (k: ConvertKind) => void
  onApply: (target: ConvertTarget, pages: number[], ocr: boolean) => void
  onClose: () => void
}) {
  const [which, setWhich] = useState<'all' | 'current' | 'range'>('all')
  const [range, setRange] = useState('')
  const [format, setFormat] = useState<'png' | 'jpeg'>('png')
  const [dpi, setDpi] = useState(150)
  const [ocr, setOcr] = useState(true)

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
  const scannedHere = scanned.filter((p) => pages.includes(p - 1))
  const showOcr = kind !== 'images' && scannedHere.length > 0

  const target: ConvertTarget = kind === 'images' ? { kind, format, dpi } : { kind }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label={TITLES[kind]} onMouseDown={(e) => e.stopPropagation()}>
        <h2>{TITLES[kind]}</h2>
        <div className="segmented">
          <button className={kind === 'word' ? 'on' : ''} onClick={() => onKind('word')}>Word</button>
          <button className={kind === 'excel' ? 'on' : ''} onClick={() => onKind('excel')}>Excel</button>
          <button className={kind === 'text' ? 'on' : ''} onClick={() => onKind('text')}>Texte</button>
          <button className={kind === 'images' ? 'on' : ''} onClick={() => onKind('images')}>Images</button>
        </div>
        <p className="small muted">{NOTES[kind]}</p>
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
            <input value={range} placeholder="1-3, 5" onFocus={() => setWhich('range')} onChange={(e) => setRange(e.target.value)} />
          </span>
        </label>
        {error && <span className="small danger-text">{error}</span>}
        {kind === 'images' && (
          <div className="field-row">
            <label className="field">
              Format
              <select value={format} onChange={(e) => setFormat(e.target.value as 'png' | 'jpeg')}>
                <option value="png">PNG (net, plus lourd)</option>
                <option value="jpeg">JPEG (plus léger)</option>
              </select>
            </label>
            <label className="field">
              Résolution
              <select value={dpi} onChange={(e) => setDpi(Number(e.target.value))}>
                <option value={72}>Écran (72 ppp)</option>
                <option value={150}>Normale (150 ppp)</option>
                <option value={300}>Impression (300 ppp)</option>
                <option value={600}>Très haute (600 ppp)</option>
              </select>
            </label>
          </div>
        )}
        {showOcr && (
          <label className="choice">
            <input type="checkbox" checked={ocr && ocrAvailable} disabled={!ocrAvailable} onChange={(e) => setOcr(e.target.checked)} />
            <span>
              Reconnaître le texte des pages scannées ({scannedHere.length})
              {!ocrAvailable && <span className="small muted"> : reconnaissance indisponible</span>}
            </span>
          </label>
        )}
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={!pages.length || !!error} onClick={() => onApply(target, pages, showOcr && ocr && ocrAvailable)}>
            {kind === 'images' ? 'Choisir le dossier…' : 'Enregistrer sous…'}
          </button>
        </div>
      </div>
    </div>
  )
}
