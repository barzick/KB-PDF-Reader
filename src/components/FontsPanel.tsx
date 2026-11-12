import { useEffect, useState } from 'react'
import { api, type FontResolution } from '../lib/api'

const ONLINE_KEY = 'kb.fonts.online'

function readOnline() {
  try {
    return localStorage.getItem(ONLINE_KEY) !== 'false'
  } catch {
    return true
  }
}

/** Fonts of the document and where the app found a complete version of each. */
export function FontsPanel({ docId, version }: { docId: string; version: number }) {
  const [fonts, setFonts] = useState<FontResolution[] | null>(null)
  const [online, setOnline] = useState(readOnline)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    api
      .resolveFonts(docId, online)
      .then((f) => live && (setFonts(f), setError(null)))
      .catch((e) => live && (setFonts([]), setError(String(e))))
    return () => {
      live = false
      setFonts(null)
    }
  }, [docId, version, online])

  const toggle = () => {
    const next = !online
    setOnline(next)
    try {
      localStorage.setItem(ONLINE_KEY, String(next))
    } catch {
      /* ignore */
    }
  }

  return (
    <div className="fonts">
      <label className="online-toggle small">
        <input type="checkbox" checked={online} onChange={toggle} />
        Télécharger les polices manquantes
      </label>
      {!fonts && <p className="muted pad">Recherche des polices…</p>}
      {error && <p className="pad small danger-text">{error}</p>}
      {fonts && fonts.length === 0 && !error && <p className="muted pad">Aucun texte dans ce document (scan ?).</p>}
      {fonts && (
        <ul className="font-list">
          {fonts.map((f) => (
            <li key={f.raw_name} title={f.raw_name}>
              <div className="font-family">
                {f.family}
                {f.weight >= 600 && <span className="tag">gras</span>}
                {f.italic && <span className="tag">italique</span>}
              </div>
              <Status f={f} />
              <div className="muted small">{f.embedded ? 'Intégrée au PDF' : 'Non intégrée au PDF'}</div>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function Status({ f }: { f: FontResolution }) {
  const s = f.source
  switch (s.kind) {
    case 'system':
      return <div className="status ok">✓ Installée sur l'ordinateur</div>
    case 'downloaded':
      return <div className="status ok">✓ Téléchargée (gratuite)</div>
    case 'substitute':
      return (
        <div className="status warn">
          ↺ Remplacée par {s.family}
          {s.same_metrics ? ' (mêmes largeurs)' : ''}
        </div>
      )
    case 'missing':
      return <div className="status bad">✕ Introuvable{f.embedded ? ' (seules les lettres du PDF)' : ''}</div>
  }
}
