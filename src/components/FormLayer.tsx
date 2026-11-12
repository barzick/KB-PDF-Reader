import { useEffect, useState } from 'react'
import type { FormField, Rect } from '../lib/api'

interface Props {
  fields: FormField[]
  pageHeight: number
  scale: number
  onFill: (name: string, value: string) => void
  onSign: (rect: Rect) => void
}

/** Fillable inputs drawn over the form fields of one page. */
export function FormLayer({ fields, pageHeight, scale, onFill, onSign }: Props) {
  return (
    <div className="form-layer">
      {fields.map((f, i) => (
        <Field key={`${f.name}-${i}`} f={f} pageHeight={pageHeight} scale={scale} onFill={onFill} onSign={onSign} />
      ))}
    </div>
  )
}

function Field({ f, pageHeight, scale, onFill, onSign }: { f: FormField } & Omit<Props, 'fields'>) {
  const [draft, setDraft] = useState(f.value)
  useEffect(() => setDraft(f.value), [f.value])

  if (f.readonly || f.kind === 'button') return null
  const [x0, y0, x1, y1] = f.rect
  const style: React.CSSProperties = {
    left: x0 * scale,
    top: (pageHeight - y1) * scale,
    width: (x1 - x0) * scale,
    height: (y1 - y0) * scale,
  }
  const h = y1 - y0
  const size = (f.font_size > 0 ? f.font_size : f.multiline ? 10 : Math.min(12, h * 0.7)) * scale
  const commit = () => {
    if (draft !== f.value) onFill(f.name, draft)
  }

  switch (f.kind) {
    case 'text':
      return f.multiline ? (
        <textarea
          className="form-field"
          style={{ ...style, fontSize: size }}
          value={draft}
          maxLength={f.max_len ?? undefined}
          title={f.name}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
        />
      ) : (
        <input
          className="form-field"
          style={{ ...style, fontSize: size }}
          value={draft}
          maxLength={f.max_len ?? undefined}
          title={f.name}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
        />
      )
    case 'checkbox':
    case 'radio':
      return (
        <button
          className={`form-toggle ${f.kind}`}
          style={style}
          title={f.kind === 'radio' ? `${f.name} : ${f.export}` : f.name}
          aria-pressed={f.checked}
          onClick={() => onFill(f.name, f.kind === 'radio' ? f.export : f.checked ? 'false' : 'true')}
        />
      )
    case 'combo':
    case 'list':
      return (
        <select
          className="form-field"
          style={{ ...style, fontSize: Math.min(size, 14 * scale) }}
          value={f.options.includes(f.value) ? f.value : ''}
          title={f.name}
          onChange={(e) => onFill(f.name, e.target.value)}
        >
          {!f.options.includes(f.value) && <option value="">{f.value || '—'}</option>}
          {f.options.map((o) => (
            <option key={o} value={o}>
              {o}
            </option>
          ))}
        </select>
      )
    case 'signature':
      return (
        <button className="form-sign" style={style} onClick={() => onSign(f.rect)}>
          ✍ Signer ici
        </button>
      )
    default:
      return null
  }
}
