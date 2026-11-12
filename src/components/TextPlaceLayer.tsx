import { useEffect, useRef, useState } from 'react'
import type { TextStyle } from '../lib/api'
import { StyleFields } from './DecorateDialog'

interface Props {
  page: number
  pageHeight: number
  scale: number
  style: TextStyle
  onStyle: (s: TextStyle) => void
  onAdd: (page: number, x: number, y: number, text: string) => Promise<void>
}

const FAMILY = { sans: 'Helvetica, Arial, sans-serif', serif: 'Times, "Times New Roman", serif', mono: 'Courier, monospace' }

/** Click on the page, type, and the text becomes part of the page. */
export function TextPlaceLayer({ page, pageHeight, scale, style, onStyle, onAdd }: Props) {
  const el = useRef<HTMLDivElement>(null)
  const area = useRef<HTMLTextAreaElement>(null)
  const [at, setAt] = useState<[number, number] | null>(null)
  const [text, setText] = useState('')

  useEffect(() => {
    if (at) requestAnimationFrame(() => area.current?.focus({ preventScroll: true }))
  }, [at])

  const submit = async () => {
    if (!at || !text.trim()) return setAt(null)
    await onAdd(page, at[0] / scale, pageHeight - at[1] / scale, text)
    setAt(null)
    setText('')
  }

  return (
    <div
      ref={el}
      className="place-layer text-place"
      onMouseDown={(e) => {
        if (e.button !== 0 || e.target !== el.current) return
        e.preventDefault()
        const r = el.current!.getBoundingClientRect()
        setText('')
        setAt([e.clientX - r.left, e.clientY - r.top])
      }}
    >
      {at && (
        <div className="text-place-box" style={{ left: at[0], top: at[1] }} onMouseDown={(e) => e.stopPropagation()}>
          <textarea
            ref={area}
            value={text}
            rows={Math.max(1, text.split('\n').length)}
            placeholder="Tapez votre texte"
            style={{
              fontFamily: FAMILY[style.family],
              fontWeight: style.bold ? 700 : 400,
              fontStyle: style.italic ? 'italic' : undefined,
              fontSize: style.size * scale,
              lineHeight: 1.2,
              color: `rgb(${style.color.join(' ')})`,
            }}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit()
              if (e.key === 'Escape') setAt(null)
            }}
          />
          <div className="annot-input">
            <StyleFields style={style} onChange={onStyle} sizes={[8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48]} />
            <div className="row">
              <button className="ghost" onClick={() => setAt(null)}>Annuler</button>
              <button className="primary" onClick={submit} disabled={!text.trim()}>Ajouter</button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
