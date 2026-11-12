import type { Rgb } from '../lib/api'

export type AnnotTool =
  | 'select'
  | 'highlight'
  | 'underline'
  | 'strike_out'
  | 'note'
  | 'free_text'
  | 'square'
  | 'circle'
  | 'arrow'
  | 'ink'
  | 'stamp'

export const MARKUP_TOOLS: AnnotTool[] = ['highlight', 'underline', 'strike_out']

const TOOLS: { tool: AnnotTool; label: string; title: string }[] = [
  { tool: 'select', label: '↖', title: 'Sélectionner une annotation (pour la lire ou la supprimer)' },
  { tool: 'highlight', label: 'Surligner', title: 'Sélectionnez du texte pour le surligner' },
  { tool: 'underline', label: 'Souligner', title: 'Sélectionnez du texte pour le souligner' },
  { tool: 'strike_out', label: 'Barrer', title: 'Sélectionnez du texte pour le barrer' },
  { tool: 'note', label: 'Note', title: 'Cliquez pour poser une note' },
  { tool: 'free_text', label: 'Texte', title: 'Tracez une zone de texte' },
  { tool: 'square', label: '▭', title: 'Rectangle' },
  { tool: 'circle', label: '◯', title: 'Ellipse' },
  { tool: 'arrow', label: '↗', title: 'Flèche' },
  { tool: 'ink', label: '✎', title: 'Dessin à main levée' },
  { tool: 'stamp', label: 'Tampon', title: 'Cliquez pour poser un tampon' },
]

export const COLORS: { color: Rgb; name: string }[] = [
  { color: [255, 230, 0], name: 'Jaune' },
  { color: [120, 220, 90], name: 'Vert' },
  { color: [90, 190, 255], name: 'Bleu clair' },
  { color: [255, 140, 200], name: 'Rose' },
  { color: [220, 30, 30], name: 'Rouge' },
  { color: [30, 80, 220], name: 'Bleu' },
  { color: [0, 0, 0], name: 'Noir' },
]

export const STAMPS = ['Approuvé', 'Payé', 'Reçu', 'Confidentiel', 'Brouillon', 'Urgent', 'Copie', 'Refusé']

interface Props {
  tool: AnnotTool
  color: Rgb
  width: number
  stamp: string
  onTool: (t: AnnotTool) => void
  onColor: (c: Rgb) => void
  onWidth: (w: number) => void
  onStamp: (s: string) => void
  onClose: () => void
}

export function AnnotToolbar({ tool, color, width, stamp, onTool, onColor, onWidth, onStamp, onClose }: Props) {
  const same = (a: Rgb, b: Rgb) => a.every((v, i) => v === b[i])
  return (
    <div className="annot-bar">
      {TOOLS.map((t) => (
        <button key={t.tool} className={tool === t.tool ? 'on' : ''} title={t.title} onClick={() => onTool(t.tool)}>
          {t.label}
        </button>
      ))}
      <span className="sep" />
      {COLORS.map((c) => (
        <button
          key={c.name}
          className={`swatch ${same(c.color, color) ? 'on' : ''}`}
          title={c.name}
          aria-label={c.name}
          style={{ background: `rgb(${c.color.join(' ')})` }}
          onClick={() => onColor(c.color)}
        />
      ))}
      {['square', 'circle', 'arrow', 'ink'].includes(tool) && (
        <label className="small" title="Épaisseur du trait">
          Trait
          <select value={width} onChange={(e) => onWidth(Number(e.target.value))}>
            {[1, 2, 3, 5, 8].map((w) => (
              <option key={w} value={w}>{w} pt</option>
            ))}
          </select>
        </label>
      )}
      {tool === 'stamp' && (
        <select value={stamp} onChange={(e) => onStamp(e.target.value)} title="Texte du tampon">
          {STAMPS.map((s) => (
            <option key={s}>{s}</option>
          ))}
        </select>
      )}
      <span className="grow" />
      <button className="icon" onClick={onClose} aria-label="Quitter l'annotation">✕</button>
    </div>
  )
}
