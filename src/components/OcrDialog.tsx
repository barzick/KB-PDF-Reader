import { useState } from 'react'

const NAMES: Record<string, string> = {
  fra: 'Français',
  eng: 'Anglais',
  deu: 'Allemand',
  ita: 'Italien',
  spa: 'Espagnol',
  por: 'Portugais',
  nld: 'Néerlandais',
}

const KEY = 'kb.ocr.languages'

function savedLanguages(available: string[]): string[] {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? 'null') as string[] | null
    const ok = s?.filter((l) => available.includes(l))
    if (ok?.length) return ok
  } catch {
    /* ignore */
  }
  return ['fra', 'eng'].filter((l) => available.includes(l))
}

export type OcrScope = 'scanned' | 'current' | 'all'

interface Props {
  languages: string[]
  scanned: number[]
  current: number
  pageCount: number
  onRun: (languages: string[], pages: number[] | null) => void
  onClose: () => void
}

const ORDER = ['fra', 'eng', 'deu', 'ita']

export function OcrDialog({ languages: available, scanned, current, pageCount, onRun, onClose }: Props) {
  const languages = [...available].sort((a, b) => {
    const ia = ORDER.indexOf(a), ib = ORDER.indexOf(b)
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b)
  })
  const [langs, setLangs] = useState<string[]>(() => savedLanguages(languages))
  const [scope, setScope] = useState<OcrScope>(scanned.length ? 'scanned' : 'current')

  const toggle = (l: string) => setLangs((ls) => (ls.includes(l) ? ls.filter((x) => x !== l) : [...ls, l]))

  const start = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(langs))
    } catch {
      /* ignore */
    }
    const pages = scope === 'scanned' ? scanned : scope === 'current' ? [current + 1] : null
    onRun(langs, pages)
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Reconnaître le texte" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Reconnaître le texte (OCR)</h2>
        <p className="muted">
          Le texte des pages scannées devient sélectionnable, cherchable et modifiable. L'aspect de la page ne change pas.
        </p>
        {languages.length === 0 ? (
          <p className="danger-text">Les modèles de langue sont absents de cette installation.</p>
        ) : (
          <fieldset className="choices">
            <legend>Langues du document</legend>
            {languages.map((l) => (
              <label key={l}>
                <input type="checkbox" checked={langs.includes(l)} onChange={() => toggle(l)} />
                {NAMES[l] ?? l}
              </label>
            ))}
          </fieldset>
        )}
        <fieldset className="choices">
          <legend>Pages</legend>
          <label>
            <input type="radio" checked={scope === 'scanned'} disabled={!scanned.length} onChange={() => setScope('scanned')} />
            Pages scannées ({scanned.length})
          </label>
          <label>
            <input type="radio" checked={scope === 'current'} onChange={() => setScope('current')} />
            Page actuelle ({current + 1})
          </label>
          <label>
            <input type="radio" checked={scope === 'all'} onChange={() => setScope('all')} />
            Tout le document ({pageCount} p.)
          </label>
        </fieldset>
        <p className="muted small">Les pages qui contiennent déjà du texte sont laissées telles quelles.</p>
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={!langs.length} onClick={start}>Reconnaître</button>
        </div>
      </div>
    </div>
  )
}
