import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { api, type Change, type Comparison, type DocMeta, type Mark } from '../lib/api'
import { PageImage } from './PageImage'

interface Doc extends DocMeta {
  version: number
}

const KIND_LABEL: Record<Change['kind'], string> = { inserted: 'Ajout', deleted: 'Suppression', replaced: 'Modification' }

const short = (s: string, n = 90) => (s.length > n ? s.slice(0, n - 1) + '…' : s)

/** One document's pages, with the changes highlighted. */
function Side({
  doc,
  side,
  changes,
  selected,
  onSelect,
}: {
  doc: Doc
  side: 'old' | 'new'
  changes: Change[]
  selected: number | null
  onSelect: (i: number) => void
}) {
  const box = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(400)

  useLayoutEffect(() => {
    const el = box.current
    if (!el) return
    const ro = new ResizeObserver(() => setWidth(Math.max(200, el.clientWidth - 32)))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Marks of each page: [change index, mark].
  const byPage = new Map<number, [number, Mark][]>()
  changes.forEach((c, i) =>
    (side === 'old' ? c.old_marks : c.new_marks).forEach((m) => {
      const l = byPage.get(m.page) ?? []
      l.push([i, m])
      byPage.set(m.page, l)
    }),
  )

  // The first mark of each change gets an id, to scroll to it.
  const seen = new Set<number>()
  const anchor = (i: number) => (seen.has(i) ? undefined : (seen.add(i), `cmp-${side}-${i}`))

  return (
    <div className="cmp-side" ref={box} data-side={side}>
      <div className="cmp-title" title={doc.name}>{side === 'old' ? 'Original' : 'Nouvelle version'} : {doc.name}</div>
      {doc.info.pages.map((p, n) => {
        const k = width / p.width
        const h = p.height * k
        return (
          <div key={n} className="cmp-page" id={`cmp-${side}-p${n}`} style={{ width, height: h }}>
            <PageImage docId={doc.id} version={doc.version} page={n} width={width} height={h} />
            {(byPage.get(n) ?? []).map(([i, m], j) => (
              <div
                key={`${i}-${j}`}
                id={anchor(i)}
                className={`cmp-mark ${changes[i].kind} ${side} ${selected === i ? 'sel' : ''}`}
                title={side === 'old' ? changes[i].old_text : changes[i].new_text}
                onClick={() => onSelect(i)}
                style={{
                  left: (m.rect[0] - p.x0) * k - 1,
                  top: (p.y0 + p.height - m.rect[3]) * k - 1,
                  width: (m.rect[2] - m.rect[0]) * k + 2,
                  height: (m.rect[3] - m.rect[1]) * k + 2,
                }}
              />
            ))}
            <span className="cmp-pno">{n + 1}</span>
          </div>
        )
      })}
    </div>
  )
}

/** Side-by-side comparison of two documents' text. */
export function CompareView({
  docs,
  initial,
  onOpenFiles,
  onClose,
}: {
  docs: Doc[]
  initial: string
  onOpenFiles: () => Promise<DocMeta[]>
  onClose: () => void
}) {
  const [oldId, setOldId] = useState(initial)
  const [newId, setNewId] = useState(() => docs.find((d) => d.id !== initial)?.id ?? '')
  const [result, setResult] = useState<Comparison | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState<number | null>(null)

  const oldDoc = docs.find((d) => d.id === oldId)
  const newDoc = docs.find((d) => d.id === newId)

  useEffect(() => {
    if (!oldDoc || !newDoc || oldDoc.id === newDoc.id) return setResult(null)
    let live = true
    setLoading(true)
    setError('')
    setSelected(null)
    api
      .compareDocs(oldDoc.id, newDoc.id)
      .then((r) => live && setResult(r))
      .catch((e) => live && setError(String(e)))
      .finally(() => live && setLoading(false))
    return () => {
      live = false
    }
  }, [oldDoc?.id, oldDoc?.version, newDoc?.id, newDoc?.version])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (!result?.changes.length) return
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        const n = result.changes.length
        go(selected === null ? 0 : (selected + (e.key === 'ArrowDown' ? 1 : n - 1)) % n)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  const go = (i: number) => {
    setSelected(i)
    const c = result?.changes[i]
    if (!c) return
    for (const side of ['old', 'new'] as const) {
      const el =
        document.getElementById(`cmp-${side}-${i}`) ?? document.getElementById(`cmp-${side}-p${side === 'old' ? c.old_page : c.new_page}`)
      el?.scrollIntoView({ block: el.classList.contains('cmp-page') ? 'start' : 'center', behavior: 'smooth' })
    }
    document.getElementById(`cmp-item-${i}`)?.scrollIntoView({ block: 'nearest' })
  }

  const openOther = async () => {
    const opened = await onOpenFiles()
    if (opened[0]) setNewId(opened[0].id)
  }

  const pick = (value: string, set: (id: string) => void) => (value === '__open' ? openOther() : set(value))

  const select = (value: string, set: (id: string) => void, label: string) => (
    <select value={value} aria-label={label} onChange={(e) => pick(e.target.value, set)}>
      {!value && <option value="">Choisir…</option>}
      {docs.map((d) => (
        <option key={d.id} value={d.id}>{d.name}</option>
      ))}
      <option value="__open">Ouvrir un autre PDF…</option>
    </select>
  )

  const counts = result?.changes.reduce(
    (a, c) => ((a[c.kind] += 1), a),
    { inserted: 0, deleted: 0, replaced: 0 } as Record<Change['kind'], number>,
  )

  return (
    <div className="compare" role="dialog" aria-label="Comparer deux documents">
      <header className="cmp-head">
        <strong>Comparer</strong>
        {select(oldId, setOldId, 'Original')}
        <button
          className="ghost"
          title="Inverser"
          onClick={() => {
            setOldId(newId)
            setNewId(oldId)
          }}
          disabled={!newId}
        >
          ⇄
        </button>
        {select(newId, setNewId, 'Nouvelle version')}
        <span className="grow" />
        <button className="ghost" onClick={onClose}>Fermer</button>
      </header>
      <div className="cmp-body">
        <aside className="cmp-list">
          {!newDoc && <p className="small muted pad">Choisissez le second document, ou ouvrez-en un autre.</p>}
          {oldDoc && newDoc && oldDoc.id === newDoc.id && <p className="small muted pad">Choisissez deux documents différents.</p>}
          {loading && <p className="small muted pad">Comparaison…</p>}
          {error && <p className="small danger-text pad">{error}</p>}
          {result && !loading && (
            <>
              <p className="small pad">
                {result.changes.length === 0
                  ? 'Aucune différence dans le texte.'
                  : `${result.changes.length} différence(s) : ${counts!.replaced} modification(s), ${counts!.inserted} ajout(s), ${counts!.deleted} suppression(s).`}
                <br />
                <span className="muted">Seul le texte est comparé ; ↑ ↓ pour passer d'une différence à l'autre.</span>
              </p>
              {result.changes.map((c, i) => (
                <button key={i} id={`cmp-item-${i}`} className={`cmp-item ${selected === i ? 'sel' : ''}`} onClick={() => go(i)}>
                  <span className={`cmp-badge ${c.kind}`}>{KIND_LABEL[c.kind]}</span>
                  <span className="small muted">p. {c.old_page + 1} → {c.new_page + 1}</span>
                  {c.old_text && <del>{short(c.old_text)}</del>}
                  {c.new_text && <ins>{short(c.new_text)}</ins>}
                </button>
              ))}
            </>
          )}
        </aside>
        {oldDoc && <Side doc={oldDoc} side="old" changes={result?.changes ?? []} selected={selected} onSelect={go} />}
        {newDoc && <Side doc={newDoc} side="new" changes={result?.changes ?? []} selected={selected} onSelect={go} />}
      </div>
    </div>
  )
}
