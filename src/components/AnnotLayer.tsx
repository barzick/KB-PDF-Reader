import { useEffect, useRef, useState } from 'react'
import { api, type AnnotInfo, type NewAnnot, type Rect, type Rgb } from '../lib/api'
import type { AnnotTool } from './AnnotToolbar'

interface Props {
  docId: string
  version: number
  page: number
  pageHeight: number
  scale: number
  tool: AnnotTool
  color: Rgb
  width: number
  stamp: string
  onAdd: (page: number, a: NewAnnot) => Promise<void>
  onDelete: (page: number, index: number) => Promise<void>
  onNote: (page: number, index: number, text: string) => Promise<void>
}

const NAMES: Record<string, string> = {
  Text: 'Note',
  FreeText: 'Zone de texte',
  Highlight: 'Surlignage',
  Underline: 'Soulignement',
  StrikeOut: 'Texte barré',
  Squiggly: 'Soulignement ondulé',
  Square: 'Rectangle',
  Circle: 'Ellipse',
  Line: 'Ligne',
  Ink: 'Dessin',
  Stamp: 'Tampon',
  Polygon: 'Polygone',
  PolyLine: 'Polyligne',
  Caret: 'Insertion',
  FileAttachment: 'Pièce jointe',
}

type Pt = [number, number]

/** Drawing and picking annotations over a page (PDF coordinates). */
export function AnnotLayer(p: Props) {
  const { docId, version, page, pageHeight, scale, tool } = p
  const [annots, setAnnots] = useState<AnnotInfo[]>([])
  const [picked, setPicked] = useState<number | null>(null)
  const [drag, setDrag] = useState<{ from: Pt; to: Pt; points: Pt[] } | null>(null)
  const [input, setInput] = useState<{ kind: 'note' | 'free_text'; rect: Rect } | null>(null)
  const [draft, setDraft] = useState('')
  const el = useRef<HTMLDivElement>(null)
  const area = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (input) requestAnimationFrame(() => area.current?.focus({ preventScroll: true }))
  }, [input])

  useEffect(() => {
    let live = true
    api.listAnnots(docId, page).then((a) => live && setAnnots(a)).catch(() => live && setAnnots([]))
    setPicked(null)
    return () => {
      live = false
    }
  }, [docId, version, page])

  useEffect(() => {
    setInput(null)
    setPicked(null)
  }, [tool])

  const pt = (e: React.MouseEvent): Pt => {
    const r = el.current!.getBoundingClientRect()
    return [(e.clientX - r.left) / scale, pageHeight - (e.clientY - r.top) / scale]
  }
  const box = (a: Pt, b: Pt): Rect => [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])]
  const css = (r: Rect) => ({
    left: r[0] * scale,
    top: (pageHeight - r[3]) * scale,
    width: (r[2] - r[0]) * scale,
    height: (r[3] - r[1]) * scale,
  })

  // Popups stay inside the page horizontally.
  const popLeft = (x: number) => Math.max(0, Math.min(x * scale, (el.current?.clientWidth ?? 1e6) - 270))

  const drawing = ['free_text', 'square', 'circle', 'arrow', 'ink'].includes(tool)

  const finish = async () => {
    if (!drag) return
    const { from, to, points } = drag
    setDrag(null)
    const r = box(from, to)
    const small = r[2] - r[0] < 4 && r[3] - r[1] < 4
    if (tool === 'free_text') {
      const rect: Rect = small ? [from[0], from[1] - 50, from[0] + 220, from[1]] : r
      setDraft('')
      setInput({ kind: 'free_text', rect })
      return
    }
    if (tool === 'ink' && points.length > 1) return p.onAdd(page, { kind: 'ink', strokes: [points], color: p.color, width: p.width })
    if (small) return
    if (tool === 'square' || tool === 'circle')
      return p.onAdd(page, { kind: tool, rect: r, color: p.color, width: p.width, fill: null })
    if (tool === 'arrow') return p.onAdd(page, { kind: 'line', from, to, color: p.color, width: p.width, arrow: true })
  }

  const submit = async () => {
    if (!input || !draft.trim()) return setInput(null)
    const color: Rgb = input.kind === 'note' ? p.color : p.color.every((c) => c > 200) ? [0, 0, 0] : p.color
    if (input.kind === 'note') await p.onAdd(page, { kind: 'note', x: input.rect[0], y: input.rect[3], text: draft, color })
    else await p.onAdd(page, { kind: 'free_text', rect: input.rect, text: draft, size: 12, color, border: false })
    setInput(null)
  }

  const current = picked !== null ? annots.find((a) => a.index === picked) : undefined

  return (
    <div
      ref={el}
      className={`annot-layer tool-${tool}`}
      style={{ pointerEvents: tool === 'select' || drawing || tool === 'note' || tool === 'stamp' ? 'auto' : 'none' }}
      onMouseDown={(e) => {
        if (e.button !== 0 || e.target !== el.current) return
        setPicked(null)
        const at = pt(e)
        if (tool === 'note') {
          // Keep the browser from moving focus back to the page after the input opens.
          e.preventDefault()
          setDraft('')
          setInput({ kind: 'note', rect: [at[0], at[1] - 20, at[0] + 20, at[1]] })
        } else if (tool === 'stamp') {
          const w = Math.max(120, p.stamp.length * 13 + 30)
          p.onAdd(page, { kind: 'stamp', rect: [at[0] - w / 2, at[1] - 22, at[0] + w / 2, at[1] + 22], label: p.stamp, color: p.color.every((c) => c > 200) ? [200, 0, 0] : p.color })
        } else if (drawing) {
          e.preventDefault()
          setDrag({ from: at, to: at, points: [at] })
        }
      }}
      onMouseMove={(e) => {
        if (!drag) return
        const at = pt(e)
        setDrag({ ...drag, to: at, points: tool === 'ink' ? [...drag.points, at] : drag.points })
      }}
      onMouseUp={finish}
      onMouseLeave={() => drag && finish()}
    >
      {/* Existing annotations: hover to read, click to select. */}
      {annots.map((a) => (
        <div
          key={a.index}
          className={`annot-hit ${picked === a.index ? 'picked' : ''}`}
          style={{ ...css(a.rect), pointerEvents: tool === 'select' || a.subtype === 'Text' ? 'auto' : 'none' }}
          title={a.contents || NAMES[a.subtype] || a.subtype}
          onMouseDown={(e) => {
            e.stopPropagation()
            setPicked(a.index)
            setDraft(a.contents)
          }}
        />
      ))}
      {drag && tool !== 'ink' && tool !== 'arrow' && (
        <div className={`annot-preview ${tool === 'circle' ? 'round' : ''}`} style={css(box(drag.from, drag.to))} />
      )}
      {drag && (tool === 'ink' || tool === 'arrow') && (
        <svg className="annot-svg">
          <polyline
            points={(tool === 'ink' ? drag.points : [drag.from, drag.to]).map(([x, y]) => `${x * scale},${(pageHeight - y) * scale}`).join(' ')}
            fill="none"
            stroke={`rgb(${p.color.join(' ')})`}
            strokeWidth={p.width * scale}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
      {input && (
        <div className="annot-input" style={{ left: popLeft(input.rect[0]), top: (pageHeight - input.rect[3]) * scale }} onMouseDown={(e) => e.stopPropagation()}>
          <textarea
            ref={area}
            rows={3}
            placeholder={input.kind === 'note' ? 'Texte de la note' : 'Texte'}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit()
              if (e.key === 'Escape') setInput(null)
            }}
          />
          <div className="row">
            <button className="ghost" onClick={() => setInput(null)}>Annuler</button>
            <button className="primary" onClick={submit}>Ajouter</button>
          </div>
        </div>
      )}
      {current && (
        <div
          className="annot-input"
          style={{ left: popLeft(current.rect[0]), top: (pageHeight - current.rect[1]) * scale + 6 }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <strong className="small">{NAMES[current.subtype] ?? current.subtype}</strong>
          {current.subtype === 'Text' ? (
            <textarea rows={3} value={draft} onChange={(e) => setDraft(e.target.value)} />
          ) : (
            current.contents && <p className="small">{current.contents}</p>
          )}
          <div className="row">
            <button className="danger" onClick={() => p.onDelete(page, current.index)}>Supprimer</button>
            <span className="grow" />
            {current.subtype === 'Text' && draft !== current.contents && (
              <button className="primary" onClick={() => p.onNote(page, current.index, draft)}>Enregistrer</button>
            )}
            <button className="ghost" onClick={() => setPicked(null)}>Fermer</button>
          </div>
        </div>
      )}
    </div>
  )
}
