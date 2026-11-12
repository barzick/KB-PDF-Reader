import { useState } from 'react'
import type { PageFormat } from '../lib/api'

const FORMATS: { value: PageFormat; label: string }[] = [
  { value: 'a4', label: 'A4' },
  { value: 'a3', label: 'A3' },
  { value: 'a5', label: 'A5' },
  { value: 'letter', label: 'Lettre US' },
  { value: 'legal', label: 'Légal US' },
]

interface Props {
  onBlank: (count: number, format: PageFormat, landscape: boolean) => void
  onImages: (format: PageFormat, margin: number) => void
  onClose: () => void
}

export function NewDialog({ onBlank, onImages, onClose }: Props) {
  const [kind, setKind] = useState<'images' | 'blank'>('images')
  const [format, setFormat] = useState<PageFormat>('a4')
  const [imgFormat, setImgFormat] = useState<PageFormat>('a4')
  const [count, setCount] = useState(1)
  const [landscape, setLandscape] = useState(false)
  const [margin, setMargin] = useState(0)

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Nouveau PDF" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Nouveau PDF</h2>
        <div className="segmented">
          <button className={kind === 'images' ? 'on' : ''} onClick={() => setKind('images')}>Depuis des images</button>
          <button className={kind === 'blank' ? 'on' : ''} onClick={() => setKind('blank')}>Document vierge</button>
        </div>
        {kind === 'images' ? (
          <>
            <p className="muted">
              Photos ou scans (JPEG, PNG, TIFF…) : une page par image, dans l'ordre choisi. Vous pourrez ensuite
              reconnaître le texte (OCR).
            </p>
            <label className="field">
              Format des pages
              <select value={imgFormat} onChange={(e) => setImgFormat(e.target.value as PageFormat)}>
                {FORMATS.map((f) => (
                  <option key={f.value} value={f.value}>{f.label}</option>
                ))}
                <option value="fit">Taille de l'image</option>
              </select>
            </label>
            <label className="field">
              Marges
              <select value={margin} onChange={(e) => setMargin(Number(e.target.value))}>
                <option value={0}>Aucune</option>
                <option value={18}>Petites (6 mm)</option>
                <option value={36}>Normales (13 mm)</option>
              </select>
            </label>
            <div className="dialog-actions">
              <button className="ghost" onClick={onClose}>Annuler</button>
              <button className="primary" onClick={() => onImages(imgFormat, margin)}>Choisir les images…</button>
            </div>
          </>
        ) : (
          <>
            <label className="field">
              Format
              <select value={format} onChange={(e) => setFormat(e.target.value as PageFormat)}>
                {FORMATS.map((f) => (
                  <option key={f.value} value={f.value}>{f.label}</option>
                ))}
              </select>
            </label>
            <label className="field">
              Orientation
              <select value={landscape ? 'l' : 'p'} onChange={(e) => setLandscape(e.target.value === 'l')}>
                <option value="p">Portrait</option>
                <option value="l">Paysage</option>
              </select>
            </label>
            <label className="field">
              Nombre de pages
              <input type="number" min={1} max={500} value={count} onChange={(e) => setCount(Math.max(1, Number(e.target.value) || 1))} />
            </label>
            <div className="dialog-actions">
              <button className="ghost" onClick={onClose}>Annuler</button>
              <button className="primary" onClick={() => onBlank(count, format, landscape)}>Créer</button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
