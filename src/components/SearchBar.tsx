import { useEffect, useRef } from 'react'

interface Props {
  query: string
  matchCase: boolean
  count: number
  index: number
  searching: boolean
  onQuery: (q: string) => void
  onMatchCase: (v: boolean) => void
  onStep: (dir: 1 | -1) => void
  onClose: () => void
}

export function SearchBar({ query, matchCase, count, index, searching, onQuery, onMatchCase, onStep, onClose }: Props) {
  const input = useRef<HTMLInputElement>(null)
  useEffect(() => {
    input.current?.focus()
    input.current?.select()
  }, [])
  return (
    <div className="search-bar" role="search">
      <input
        ref={input}
        placeholder="Rechercher dans le document"
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') (e.preventDefault(), onStep(e.shiftKey ? -1 : 1))
          else if (e.key === 'Escape') onClose()
        }}
      />
      <span className="muted small count">
        {searching ? '…' : query.trim() ? (count ? `${index + 1} / ${count}` : 'Aucun résultat') : ''}
      </span>
      <button className="icon" onClick={() => onStep(-1)} disabled={!count} aria-label="Précédent">↑</button>
      <button className="icon" onClick={() => onStep(1)} disabled={!count} aria-label="Suivant">↓</button>
      <label className="small" title="Respecter majuscules et accents">
        <input type="checkbox" checked={matchCase} onChange={(e) => onMatchCase(e.target.checked)} /> Aa
      </label>
      <button className="icon" onClick={onClose} aria-label="Fermer la recherche">✕</button>
    </div>
  )
}
