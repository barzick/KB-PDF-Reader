import { useEffect, useState } from 'react'
import { api, type FontUsage } from '../lib/api'

export function FontsPanel({ docId, version }: { docId: string; version: number }) {
  const [fonts, setFonts] = useState<FontUsage[] | null>(null)
  useEffect(() => {
    setFonts(null)
    api.docFonts(docId).then(setFonts).catch(() => setFonts([]))
  }, [docId, version])

  if (!fonts) return <p className="muted pad">Analyse des polices…</p>
  if (fonts.length === 0) return <p className="muted pad">Aucun texte dans ce document (scan ?).</p>
  return (
    <ul className="font-list">
      {fonts.map((f) => (
        <li key={f.raw_name}>
          <div className="font-family">
            {f.family}
            {f.bold && <span className="tag">gras</span>}
            {f.italic && <span className="tag">italique</span>}
          </div>
          <div className="muted small" title={f.raw_name}>
            {f.embedded ? 'Intégrée au PDF' : 'Non intégrée'} · p. {summarize(f.pages)}
          </div>
        </li>
      ))}
    </ul>
  )
}

function summarize(pages: number[]) {
  if (pages.length <= 4) return pages.join(', ')
  return `${pages[0]}–${pages[pages.length - 1]} (${pages.length})`
}
