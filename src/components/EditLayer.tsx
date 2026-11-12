import { useEffect, useRef, useState } from 'react'
import { api, type EditMode, type TextBlock } from '../lib/api'

interface Props {
  docId: string
  version: number
  page: number
  pageHeight: number
  scale: number
  onEdit: (page: number, block: TextBlock, text: string, mode: EditMode) => Promise<boolean>
}

/** Clickable paragraphs drawn over a page while "Modifier le texte" is on. */
export function EditLayer({ docId, version, page, pageHeight, scale, onEdit }: Props) {
  const [blocks, setBlocks] = useState<TextBlock[]>([])
  const [editing, setEditing] = useState<number | null>(null)

  useEffect(() => {
    let live = true
    api.pageBlocks(docId, page).then((b) => live && setBlocks(b)).catch(() => live && setBlocks([]))
    return () => {
      live = false
      setEditing(null)
    }
  }, [docId, version, page])

  return (
    <div className="edit-layer">
      {blocks.map((b, i) => {
        const style = {
          left: b.left * scale - 3,
          top: (pageHeight - b.top) * scale - 3,
          width: (b.right - b.left) * scale + 6,
          height: (b.top - b.bottom) * scale + 6,
        }
        if (editing === i) {
          return (
            <Editor
              key={i}
              block={b}
              scale={scale}
              pageHeight={pageHeight}
              onCancel={() => setEditing(null)}
              onSave={async (text, mode) => {
                const ok = await onEdit(page, b, text, mode)
                if (ok) setEditing(null)
              }}
            />
          )
        }
        return (
          <button
            key={i}
            className="text-block"
            style={style}
            title="Cliquer pour modifier ce paragraphe"
            onClick={() => setEditing(i)}
          />
        )
      })}
    </div>
  )
}

function Editor({
  block,
  scale,
  pageHeight,
  onCancel,
  onSave,
}: {
  block: TextBlock
  scale: number
  pageHeight: number
  onCancel: () => void
  onSave: (text: string, mode: EditMode) => Promise<void>
}) {
  const [text, setText] = useState(block.text)
  const [shift, setShift] = useState(false)
  const [saving, setSaving] = useState(false)
  const area = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    const el = area.current
    if (!el) return
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
  }, [])

  // Grow with the content.
  useEffect(() => {
    const el = area.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }, [text])

  const save = async () => {
    if (text === block.text) return onCancel()
    setSaving(true)
    await onSave(text, shift ? 'shift_below' : 'exact')
    setSaving(false)
  }

  const [r, g, b] = block.color
  const bold = /bold|black|heavy|semibold/i.test(block.font)
  const italic = /italic|oblique/i.test(block.font)
  const width = Math.max(block.right - block.x, 160 / scale) * scale

  return (
    <div
      className="block-editor"
      style={{ left: block.x * scale - 4, top: (pageHeight - block.top) * scale - 4, width: width + 8 }}
      onMouseDown={(e) => e.stopPropagation()}
    >
      <textarea
        ref={area}
        value={text}
        disabled={saving}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel()
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) (e.preventDefault(), save())
        }}
        style={{
          fontSize: block.size * scale,
          lineHeight: `${block.line_height * scale}px`,
          fontWeight: bold ? 700 : 400,
          fontStyle: italic ? 'italic' : 'normal',
          color: `rgb(${r} ${g} ${b})`,
        }}
      />
      <div className="block-editor-bar">
        <label className="small" title="Déplace le contenu en dessous si le paragraphe change de nombre de lignes">
          <input type="checkbox" checked={shift} onChange={(e) => setShift(e.target.checked)} /> Décaler la suite
        </label>
        <span className="grow" />
        <button className="ghost" onClick={onCancel} disabled={saving}>Annuler</button>
        <button className="primary" onClick={save} disabled={saving} title="Ctrl/⌘ + Entrée">
          {saving ? 'Enregistrement…' : 'Valider'}
        </button>
      </div>
    </div>
  )
}
