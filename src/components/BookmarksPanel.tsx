import { useEffect, useState } from 'react'
import { api, type Bookmark } from '../lib/api'

interface Props {
  docId: string
  version: number
  current: number
  onGo: (page: number) => void
  onSave: (list: Bookmark[]) => Promise<void>
}

type Path = number[]

const clone = (l: Bookmark[]): Bookmark[] => JSON.parse(JSON.stringify(l))

/** The list holding the item at `path`, and its index there. */
function locate(root: Bookmark[], path: Path): [Bookmark[], number] {
  let list = root
  for (const i of path.slice(0, -1)) list = list[i].children
  return [list, path[path.length - 1]]
}

/** Signets: navigate, add, rename, move, nest and remove. */
export function BookmarksPanel({ docId, version, current, onGo, onSave }: Props) {
  const [list, setList] = useState<Bookmark[]>([])
  const [loaded, setLoaded] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [open, setOpen] = useState<Set<string>>(new Set())

  useEffect(() => {
    let live = true
    api
      .getBookmarks(docId)
      .then((l) => live && (setList(l), setLoaded(true)))
      .catch(() => live && (setList([]), setLoaded(true)))
    return () => {
      live = false
    }
  }, [docId, version])

  const change = (fn: (l: Bookmark[]) => void) => {
    const next = clone(list)
    fn(next)
    setList(next)
    onSave(next)
  }

  const add = () => {
    const title = `Page ${current + 1}`
    const next = clone(list)
    next.push({ title, page: current, children: [] })
    setList(next)
    setEditing(String(next.length - 1))
    setDraft(title)
  }

  const commitRename = (path: Path) => {
    setEditing(null)
    change((l) => {
      const [arr, i] = locate(l, path)
      arr[i].title = draft.trim() || arr[i].title
    })
  }

  const move = (path: Path, dir: -1 | 1) =>
    change((l) => {
      const [arr, i] = locate(l, path)
      const j = i + dir
      if (j < 0 || j >= arr.length) return
      ;[arr[i], arr[j]] = [arr[j], arr[i]]
    })

  const indent = (path: Path) =>
    change((l) => {
      const [arr, i] = locate(l, path)
      if (i === 0) return
      const [item] = arr.splice(i, 1)
      arr[i - 1].children.push(item)
    })

  const outdent = (path: Path) =>
    change((l) => {
      if (path.length < 2) return
      const [arr, i] = locate(l, path)
      const [item] = arr.splice(i, 1)
      const [parentArr, pi] = locate(l, path.slice(0, -1))
      parentArr.splice(pi + 1, 0, item)
    })

  const remove = (path: Path) =>
    change((l) => {
      const [arr, i] = locate(l, path)
      arr.splice(i, 1)
    })

  const render = (items: Bookmark[], base: Path) =>
    items.map((b, i) => {
      const path = [...base, i]
      const key = path.join('.')
      const isOpen = open.has(key)
      return (
        <li key={key}>
          <div className={`bm-row ${b.page === current ? 'here' : ''}`}>
            {b.children.length > 0 ? (
              <button
                className="bm-twist"
                aria-label={isOpen ? 'Replier' : 'Déplier'}
                onClick={() => {
                  const n = new Set(open)
                  if (isOpen) n.delete(key)
                  else n.add(key)
                  setOpen(n)
                }}
              >
                {isOpen ? '▾' : '▸'}
              </button>
            ) : (
              <span className="bm-twist" />
            )}
            {editing === key ? (
              <input
                autoFocus
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={() => commitRename(path)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitRename(path)
                  if (e.key === 'Escape') setEditing(null)
                }}
              />
            ) : (
              <button
                className="bm-title"
                title={b.page !== null ? `Page ${b.page + 1} — double-clic pour renommer` : 'Double-clic pour renommer'}
                onClick={() => b.page !== null && onGo(b.page)}
                onDoubleClick={() => {
                  setEditing(key)
                  setDraft(b.title)
                }}
              >
                {b.title || '(sans titre)'}
              </button>
            )}
            <span className="bm-tools">
              <button title="Monter" onClick={() => move(path, -1)}>↑</button>
              <button title="Descendre" onClick={() => move(path, 1)}>↓</button>
              <button title="Mettre sous le signet précédent" onClick={() => indent(path)}>→</button>
              {path.length > 1 && (
                <button title="Remonter d'un niveau" onClick={() => outdent(path)}>←</button>
              )}
              <button
                title="Pointer vers la page actuelle"
                onClick={() =>
                  change((l) => {
                    const [arr, j] = locate(l, path)
                    arr[j].page = current
                  })
                }
              >
                ⌖
              </button>
              <button title="Supprimer" className="danger-text" onClick={() => remove(path)}>✕</button>
            </span>
          </div>
          {b.children.length > 0 && isOpen && <ul>{render(b.children, path)}</ul>}
        </li>
      )
    })

  return (
    <div className="bookmarks">
      <div className="bm-head">
        <button className="primary" onClick={add}>+ Ajouter un signet</button>
      </div>
      {loaded && list.length === 0 && <p className="small muted pad">Aucun signet. Ajoutez-en pour retrouver vite les parties du document.</p>}
      <ul className="bm-tree">{render(list, [])}</ul>
    </div>
  )
}
