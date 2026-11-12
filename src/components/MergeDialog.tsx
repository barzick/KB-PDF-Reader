import { useState } from 'react'
import type { DocMeta } from '../lib/api'

interface Props {
  docs: DocMeta[]
  onAddFiles: () => Promise<DocMeta[]>
  onMerge: (ids: string[], name: string) => void
  onClose: () => void
}

export function MergeDialog({ docs, onAddFiles, onMerge, onClose }: Props) {
  const [order, setOrder] = useState<string[]>(docs.map((d) => d.id))
  const [extra, setExtra] = useState<DocMeta[]>([])
  const [name, setName] = useState('Fusion.pdf')
  const all = [...docs, ...extra]
  const byId = (id: string) => all.find((d) => d.id === id)!

  const move = (i: number, delta: number) => {
    const j = i + delta
    if (j < 0 || j >= order.length) return
    const next = [...order]
    ;[next[i], next[j]] = [next[j], next[i]]
    setOrder(next)
  }

  const add = async () => {
    const added = await onAddFiles()
    setExtra((e) => [...e, ...added])
    setOrder((o) => [...o, ...added.map((d) => d.id)])
  }

  const total = order.reduce((n, id) => n + byId(id).info.page_count, 0)

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Fusionner des PDF" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Fusionner des PDF</h2>
        <p className="muted">Les documents seront assemblés dans cet ordre.</p>
        <ol className="merge-list">
          {order.map((id, i) => {
            const d = byId(id)
            return (
              <li key={id}>
                <span className="merge-name" title={d.path ?? d.name}>{d.name}</span>
                <span className="muted">{d.info.page_count} p.</span>
                <button className="icon" onClick={() => move(i, -1)} disabled={i === 0} aria-label="Monter">↑</button>
                <button className="icon" onClick={() => move(i, 1)} disabled={i === order.length - 1} aria-label="Descendre">↓</button>
                <button className="icon" onClick={() => setOrder(order.filter((x) => x !== id))} aria-label="Retirer">✕</button>
              </li>
            )
          })}
        </ol>
        <button className="ghost" onClick={add}>+ Ajouter des fichiers…</button>
        <label className="field">
          Nom du nouveau document
          <input value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="dialog-actions">
          <span className="muted">{total} pages au total</span>
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button
            className="primary"
            disabled={order.length < 2}
            onClick={() => onMerge(order, name.toLowerCase().endsWith('.pdf') ? name : `${name}.pdf`)}
          >
            Fusionner
          </button>
        </div>
      </div>
    </div>
  )
}
