import { useState } from 'react'
import { parsePages, type HeaderFooter, type Rgb, type TextStyle, type Watermark } from '../lib/api'

const COLORS: { color: Rgb; name: string }[] = [
  { color: [0, 0, 0], name: 'Noir' },
  { color: [100, 100, 100], name: 'Gris' },
  { color: [200, 30, 30], name: 'Rouge' },
  { color: [30, 80, 200], name: 'Bleu' },
  { color: [20, 120, 60], name: 'Vert' },
]

const same = (a: Rgb, b: Rgb) => a.every((v, i) => v === b[i])

/** Font, size, weight and colour controls shared by the dialogs. */
export function StyleFields({ style, onChange, sizes }: { style: TextStyle; onChange: (s: TextStyle) => void; sizes: number[] }) {
  return (
    <div className="style-fields">
      <select value={style.family} onChange={(e) => onChange({ ...style, family: e.target.value as TextStyle['family'] })} aria-label="Police">
        <option value="sans">Helvetica</option>
        <option value="serif">Times</option>
        <option value="mono">Courier</option>
      </select>
      <select value={style.size} onChange={(e) => onChange({ ...style, size: Number(e.target.value) })} aria-label="Taille">
        {sizes.map((s) => (
          <option key={s} value={s}>{s} pt</option>
        ))}
      </select>
      <button className={style.bold ? 'on' : ''} onClick={() => onChange({ ...style, bold: !style.bold })} title="Gras">
        <b>G</b>
      </button>
      <button className={style.italic ? 'on' : ''} onClick={() => onChange({ ...style, italic: !style.italic })} title="Italique">
        <i>I</i>
      </button>
      {COLORS.map((c) => (
        <button
          key={c.name}
          className={`swatch ${same(c.color, style.color) ? 'on' : ''}`}
          style={{ background: `rgb(${c.color.join(' ')})` }}
          title={c.name}
          aria-label={c.name}
          onClick={() => onChange({ ...style, color: c.color })}
        />
      ))}
    </div>
  )
}

function PagesField({ value, onChange, count }: { value: string; onChange: (v: string) => void; count: number }) {
  let error = ''
  try {
    parsePages(value, count)
  } catch (e) {
    error = (e as Error).message
  }
  return (
    <label className="field">
      Pages
      <input placeholder={`Toutes (1-${count})`} value={value} onChange={(e) => onChange(e.target.value)} />
      {error ? <span className="small danger-text">{error}</span> : <span className="small muted">Exemple : 1-3, 5. Vide = toutes.</span>}
    </label>
  )
}

export function WatermarkDialog({ pageCount, onApply, onClose }: { pageCount: number; onApply: (w: Watermark) => void; onClose: () => void }) {
  const [text, setText] = useState('CONFIDENTIEL')
  const [style, setStyle] = useState<TextStyle>({ family: 'sans', bold: true, italic: false, size: 60, color: [200, 30, 30], opacity: 0.25 })
  const [angle, setAngle] = useState(45)
  const [pages, setPages] = useState('')

  const apply = () => {
    try {
      onApply({ text, style, angle, pages: parsePages(pages, pageCount) })
    } catch {
      /* shown under the field */
    }
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Filigrane" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Filigrane</h2>
        <label className="field">
          Texte (une ligne par ligne)
          <textarea rows={2} value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        <StyleFields style={style} onChange={setStyle} sizes={[24, 36, 48, 60, 72, 96, 120]} />
        <label className="field">
          Opacité : {Math.round(style.opacity * 100)} %
          <input type="range" min={5} max={100} value={Math.round(style.opacity * 100)} onChange={(e) => setStyle({ ...style, opacity: Number(e.target.value) / 100 })} />
        </label>
        <div className="segmented">
          {[
            [0, 'Horizontal'],
            [45, 'Diagonale ↗'],
            [-45, 'Diagonale ↘'],
            [90, 'Vertical'],
          ].map(([a, label]) => (
            <button key={a} className={angle === a ? 'on' : ''} onClick={() => setAngle(a as number)}>
              {label}
            </button>
          ))}
        </div>
        <div className="wm-preview">
          <span
            style={{
              transform: `rotate(${-angle}deg)`,
              color: `rgb(${style.color.join(' ')} / ${style.opacity})`,
              fontFamily: style.family === 'serif' ? 'Times, serif' : style.family === 'mono' ? 'Courier, monospace' : 'Helvetica, Arial, sans-serif',
              fontWeight: style.bold ? 700 : 400,
              fontStyle: style.italic ? 'italic' : undefined,
              fontSize: style.size * 0.28,
            }}
          >
            {text.split('\n').map((l, i) => (
              <div key={i}>{l}</div>
            ))}
          </span>
        </div>
        <PagesField value={pages} onChange={setPages} count={pageCount} />
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={!text.trim()} onClick={apply}>Ajouter le filigrane</button>
        </div>
      </div>
    </div>
  )
}

const today = () => new Date().toLocaleDateString('fr-CH', { day: '2-digit', month: '2-digit', year: 'numeric' })

export function HeaderFooterDialog({
  pageCount,
  fileName,
  numbersOnly,
  onApply,
  onClose,
}: {
  pageCount: number
  fileName: string
  numbersOnly: boolean
  onApply: (h: HeaderFooter) => void
  onClose: () => void
}) {
  const [header, setHeader] = useState<[string, string, string]>(['', '', ''])
  const [footer, setFooter] = useState<[string, string, string]>(['', numbersOnly ? 'Page {page} / {pages}' : '', ''])
  const [style, setStyle] = useState<TextStyle>({ family: 'sans', bold: false, italic: false, size: 10, color: [0, 0, 0], opacity: 1 })
  const [margin, setMargin] = useState(28)
  const [start, setStart] = useState(1)
  const [pages, setPages] = useState('')

  const slot = (row: 'header' | 'footer', i: number) => {
    const v = row === 'header' ? header : footer
    const set = row === 'header' ? setHeader : setFooter
    return (
      <input
        value={v[i]}
        placeholder={['Gauche', 'Centre', 'Droite'][i]}
        onChange={(e) => {
          const n = [...v] as [string, string, string]
          n[i] = e.target.value
          set(n)
        }}
      />
    )
  }

  const apply = () => {
    try {
      onApply({ header, footer, style, margin, start, pages: parsePages(pages, pageCount), date: today(), file: fileName })
    } catch {
      /* shown under the field */
    }
  }

  const presets: [string, string][] = [
    ['1, 2, 3…', '{page}'],
    ['Page 1 / 3', 'Page {page} / {pages}'],
    ['- 1 -', '- {page} -'],
  ]

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog hf-dialog" role="dialog" aria-label="En-tête et pied de page" onMouseDown={(e) => e.stopPropagation()}>
        <h2>{numbersOnly ? 'Numéros de page' : 'En-tête et pied de page'}</h2>
        {numbersOnly ? (
          <>
            <div className="segmented">
              {presets.map(([label, v]) => (
                <button key={v} className={footer[1] === v ? 'on' : ''} onClick={() => setFooter(['', v, ''])}>
                  {label}
                </button>
              ))}
            </div>
            <div className="hf-grid">
              <span className="small muted">Pied de page</span>
              {slot('footer', 0)}
              {slot('footer', 1)}
              {slot('footer', 2)}
            </div>
          </>
        ) : (
          <div className="hf-grid">
            <span className="small muted">En-tête</span>
            {slot('header', 0)}
            {slot('header', 1)}
            {slot('header', 2)}
            <span className="small muted">Pied de page</span>
            {slot('footer', 0)}
            {slot('footer', 1)}
            {slot('footer', 2)}
          </div>
        )}
        <p className="small muted">
          Codes : {'{page}'} numéro, {'{pages}'} nombre de pages, {'{date}'} date du jour, {'{file}'} nom du fichier.
        </p>
        <StyleFields style={style} onChange={setStyle} sizes={[7, 8, 9, 10, 11, 12, 14, 16]} />
        <div className="hf-row">
          <label className="field">
            Marge
            <select value={margin} onChange={(e) => setMargin(Number(e.target.value))}>
              <option value={14}>Petite (5 mm)</option>
              <option value={28}>Normale (10 mm)</option>
              <option value={42}>Grande (15 mm)</option>
            </select>
          </label>
          <label className="field">
            Premier numéro
            <input type="number" min={0} value={start} onChange={(e) => setStart(Math.max(0, Number(e.target.value)))} />
          </label>
        </div>
        <PagesField value={pages} onChange={setPages} count={pageCount} />
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={![...header, ...footer].some((s) => s.trim())} onClick={apply}>
            Ajouter
          </button>
        </div>
      </div>
    </div>
  )
}
