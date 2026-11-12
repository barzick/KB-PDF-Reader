import { useEffect, useRef, useState } from 'react'
import type { PageText, SearchHit } from '../lib/api'
import { charAt, getPageText, lineRects, wordAround } from '../lib/pageText'

export interface Selection {
  page: number
  start: number
  end: number
}

interface Props {
  docId: string
  version: number
  page: number
  pageHeight: number
  scale: number
  hits: { hit: SearchHit; active: boolean }[]
  selection: Selection | null
  onSelect: (s: Selection | null, text: PageText | null) => void
  onCopy: () => void
  /** Called when the mouse is released after selecting (annotation tools). */
  onSelectEnd?: () => void
  /** Hide the "Copier" chip (annotation tools act on the selection instead). */
  noChip?: boolean
}

/** Text selection and search highlights over a page. */
export function TextLayer({ docId, version, page, pageHeight, scale, hits, selection, onSelect, onCopy, onSelectEnd, noChip }: Props) {
  const [dragging, setDragging] = useState(false)
  const [text, setText] = useState<PageText | null>(null)
  const anchor = useRef<number | null>(null)
  const el = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let live = true
    setText(null)
    getPageText(docId, version, page).then((t) => live && setText(t))
    return () => {
      live = false
    }
  }, [docId, version, page])

  const point = (e: React.MouseEvent) => {
    const r = el.current!.getBoundingClientRect()
    return { x: (e.clientX - r.left) / scale, y: pageHeight - (e.clientY - r.top) / scale }
  }

  const hasText = !!text && text.boxes.some((b) => b[2] > b[0])
  const sel = selection?.page === page && text ? selection : null
  const selRects = sel
    ? lineRects(text!.boxes.slice(Math.min(sel.start, sel.end), Math.max(sel.start, sel.end) + 1))
    : []

  const rect = (b: [number, number, number, number], cls: string, key: string) => (
    <div
      key={key}
      className={cls}
      style={{
        left: b[0] * scale - 1,
        top: (pageHeight - b[3]) * scale - 1,
        width: (b[2] - b[0]) * scale + 2,
        height: (b[3] - b[1]) * scale + 2,
      }}
    />
  )

  return (
    <div
      ref={el}
      className={`text-layer ${hasText ? 'has-text' : ''}`}
      onMouseDown={(e) => {
        if (e.button !== 0 || !text || !hasText) return
        const p = point(e)
        const i = charAt(text, p.x, p.y)
        if (i === null) return onSelect(null, null)
        e.preventDefault()
        if (e.detail === 2) {
          const [a, b] = wordAround(text, i)
          anchor.current = null
          onSelect({ page, start: a, end: b }, text)
          window.setTimeout(() => onSelectEnd?.(), 0)
          return
        }
        if (e.shiftKey && sel) {
          anchor.current = sel.start
          return onSelect({ page, start: sel.start, end: i }, text)
        }
        anchor.current = i
        setDragging(true)
        onSelect(null, null)
      }}
      onMouseMove={(e) => {
        if (anchor.current === null || !text || !(e.buttons & 1)) return
        const p = point(e)
        const i = charAt(text, p.x, p.y)
        if (i !== null && (i !== anchor.current || sel)) onSelect({ page, start: anchor.current, end: i }, text)
      }}
      onMouseUp={() => {
        const was = anchor.current !== null
        anchor.current = null
        setDragging(false)
        if (was) window.setTimeout(() => onSelectEnd?.(), 0)
      }}
    >
      {hits.flatMap(({ hit, active }, n) =>
        hit.rects.map((b, k) => rect(b, active ? 'hit active' : 'hit', `h${n}-${k}`)),
      )}
      {selRects.map((b, k) => rect(b, 'sel', `s${k}`))}
      {!dragging && !noChip && selRects.length > 0 && (
        <button
          className="primary copy-chip small"
          style={{ left: selRects[0][0] * scale, top: Math.max(0, (pageHeight - selRects[0][3]) * scale - 30) }}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={onCopy}
        >
          Copier
        </button>
      )}
    </div>
  )
}
