import { useEffect, useRef, useState } from 'react'

export interface PlacedImage {
  /** PNG or JPEG bytes. */
  data: number[]
  /** Data URL for previews. */
  url: string
  width: number
  height: number
}

interface Props {
  onClose: () => void
  onPick: (img: PlacedImage) => void
}

const STORE = 'kb.signatures'
const INKS = ['#111111', '#1d3fbf', '#0b6b3a']
const FONTS = [
  { family: 'KB Great Vibes', label: 'Élégante' },
  { family: 'KB Dancing Script', label: 'Manuscrite' },
  { family: 'Georgia', label: 'Classique', italic: true },
]

function saved(): string[] {
  try {
    return JSON.parse(localStorage.getItem(STORE) ?? '[]')
  } catch {
    return []
  }
}

function remember(url: string) {
  try {
    const list = [url, ...saved().filter((u) => u !== url)].slice(0, 6)
    localStorage.setItem(STORE, JSON.stringify(list))
  } catch {
    /* storage unavailable: nothing to remember */
  }
}

function forget(url: string) {
  try {
    localStorage.setItem(STORE, JSON.stringify(saved().filter((u) => u !== url)))
  } catch {
    /* ignore */
  }
}

/** Crops a canvas to its non-transparent pixels (with a small margin). */
function crop(src: HTMLCanvasElement): HTMLCanvasElement | null {
  const ctx = src.getContext('2d')!
  const { width: w, height: h } = src
  const px = ctx.getImageData(0, 0, w, h).data
  let [x0, y0, x1, y1] = [w, h, -1, -1]
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++)
      if (px[(y * w + x) * 4 + 3] > 8) {
        if (x < x0) x0 = x
        if (x > x1) x1 = x
        if (y < y0) y0 = y
        if (y > y1) y1 = y
      }
  if (x1 < 0) return null
  const m = 6
  x0 = Math.max(0, x0 - m)
  y0 = Math.max(0, y0 - m)
  x1 = Math.min(w - 1, x1 + m)
  y1 = Math.min(h - 1, y1 + m)
  const out = document.createElement('canvas')
  out.width = x1 - x0 + 1
  out.height = y1 - y0 + 1
  out.getContext('2d')!.drawImage(src, x0, y0, out.width, out.height, 0, 0, out.width, out.height)
  return out
}

export async function fromDataUrl(url: string): Promise<PlacedImage> {
  const img = new Image()
  img.src = url
  await img.decode()
  const bytes = Uint8Array.from(atob(url.split(',')[1]), (c) => c.charCodeAt(0))
  return { data: Array.from(bytes), url, width: img.naturalWidth, height: img.naturalHeight }
}

/** Reads an image file; optionally turns its light background transparent. */
export async function fromFile(file: File, clearBackground: boolean): Promise<PlacedImage> {
  const url = await new Promise<string>((ok, ko) => {
    const r = new FileReader()
    r.onload = () => ok(r.result as string)
    r.onerror = () => ko(r.error)
    r.readAsDataURL(file)
  })
  if (!clearBackground) {
    const img = await fromDataUrl(url)
    // Keep PNG/JPEG as they are; convert anything else to PNG.
    if (/^data:image\/(png|jpeg)/.test(url)) return img
  }
  const img = new Image()
  img.src = url
  await img.decode()
  const c = document.createElement('canvas')
  c.width = img.naturalWidth
  c.height = img.naturalHeight
  const ctx = c.getContext('2d')!
  ctx.drawImage(img, 0, 0)
  if (clearBackground) {
    const d = ctx.getImageData(0, 0, c.width, c.height)
    for (let i = 0; i < d.data.length; i += 4) {
      const l = 0.299 * d.data[i] + 0.587 * d.data[i + 1] + 0.114 * d.data[i + 2]
      // Paper becomes transparent, ink keeps its colour with a soft edge.
      d.data[i + 3] = Math.min(d.data[i + 3], Math.max(0, Math.min(255, (225 - l) * 4)))
    }
    ctx.putImageData(d, 0, 0)
  }
  return fromDataUrl((crop(c) ?? c).toDataURL('image/png'))
}

/** Create, reuse or import a signature. */
export function SignatureDialog({ onClose, onPick }: Props) {
  const [tab, setTab] = useState<'draw' | 'type' | 'image'>('draw')
  const [list, setList] = useState(saved)
  const [ink, setInk] = useState(INKS[0])
  const [name, setName] = useState('')
  const [font, setFont] = useState(0)
  const [clearBg, setClearBg] = useState(true)
  const [empty, setEmpty] = useState(true)
  const [keep, setKeep] = useState(true)
  const pad = useRef<HTMLCanvasElement>(null)
  const last = useRef<[number, number] | null>(null)

  useEffect(() => {
    document.fonts?.load(`40px "KB Great Vibes"`).catch(() => {})
    document.fonts?.load(`40px "KB Dancing Script"`).catch(() => {})
  }, [])

  const clear = () => {
    const c = pad.current
    if (!c) return
    c.getContext('2d')!.clearRect(0, 0, c.width, c.height)
    setEmpty(true)
  }

  const point = (e: React.PointerEvent<HTMLCanvasElement>): [number, number] => {
    const r = e.currentTarget.getBoundingClientRect()
    return [((e.clientX - r.left) * e.currentTarget.width) / r.width, ((e.clientY - r.top) * e.currentTarget.height) / r.height]
  }

  const use = async (url: string) => {
    if (keep) remember(url)
    onPick(await fromDataUrl(url))
  }

  const typed = () => {
    const f = FONTS[font]
    const c = document.createElement('canvas')
    c.width = 1400
    c.height = 300
    const ctx = c.getContext('2d')!
    ctx.fillStyle = ink
    ctx.font = `${f.italic ? 'italic ' : ''}150px "${f.family}"`
    ctx.textBaseline = 'middle'
    ctx.fillText(name.trim(), 30, 160, 1340)
    return crop(c)
  }

  const confirm = async () => {
    if (tab === 'draw' && pad.current) {
      const c = crop(pad.current)
      if (c) await use(c.toDataURL('image/png'))
    } else if (tab === 'type' && name.trim()) {
      const c = typed()
      if (c) await use(c.toDataURL('image/png'))
    }
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog sign-dialog" role="dialog" aria-label="Signature" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Signature</h2>
        {list.length > 0 && (
          <>
            <p className="small muted">Signatures enregistrées : cliquez pour l'utiliser.</p>
            <div className="sign-saved">
              {list.map((u) => (
                <div key={u} className="sign-thumb">
                  <button onClick={() => fromDataUrl(u).then(onPick)} title="Utiliser">
                    <img src={u} alt="signature" />
                  </button>
                  <button
                    className="icon"
                    aria-label="Oublier"
                    title="Oublier cette signature"
                    onClick={() => {
                      forget(u)
                      setList(saved())
                    }}
                  >
                    ✕
                  </button>
                </div>
              ))}
            </div>
          </>
        )}
        <div className="segmented">
          <button className={tab === 'draw' ? 'on' : ''} onClick={() => setTab('draw')}>Dessiner</button>
          <button className={tab === 'type' ? 'on' : ''} onClick={() => setTab('type')}>Écrire</button>
          <button className={tab === 'image' ? 'on' : ''} onClick={() => setTab('image')}>Image</button>
        </div>

        {tab !== 'image' && (
          <div className="sign-row">
            <span className="small muted">Encre</span>
            {INKS.map((c) => (
              <button key={c} className={`swatch ${ink === c ? 'on' : ''}`} style={{ background: c }} aria-label={c} onClick={() => setInk(c)} />
            ))}
          </div>
        )}

        {tab === 'draw' && (
          <div className="sign-pad-wrap">
            <canvas
              ref={pad}
              className="sign-pad"
              width={1000}
              height={300}
              onPointerDown={(e) => {
                e.currentTarget.setPointerCapture(e.pointerId)
                last.current = point(e)
                const ctx = e.currentTarget.getContext('2d')!
                const [x, y] = last.current
                ctx.fillStyle = ink
                ctx.beginPath()
                ctx.arc(x, y, 2.5, 0, Math.PI * 2)
                ctx.fill()
                setEmpty(false)
              }}
              onPointerMove={(e) => {
                if (!last.current) return
                const ctx = e.currentTarget.getContext('2d')!
                const p = point(e)
                ctx.strokeStyle = ink
                ctx.lineWidth = 5
                ctx.lineCap = 'round'
                ctx.lineJoin = 'round'
                ctx.beginPath()
                ctx.moveTo(...last.current)
                ctx.lineTo(...p)
                ctx.stroke()
                last.current = p
              }}
              onPointerUp={() => (last.current = null)}
            />
            <span className="sign-line">Signez ici</span>
            {!empty && (
              <button className="ghost sign-clear" onClick={clear}>
                Effacer
              </button>
            )}
          </div>
        )}

        {tab === 'type' && (
          <>
            <input autoFocus placeholder="Votre nom" value={name} onChange={(e) => setName(e.target.value)} />
            <div className="sign-fonts">
              {FONTS.map((f, i) => (
                <button
                  key={f.family}
                  className={font === i ? 'on' : ''}
                  style={{ fontFamily: `"${f.family}"`, fontStyle: f.italic ? 'italic' : undefined, color: ink }}
                  onClick={() => setFont(i)}
                  title={f.label}
                >
                  {name.trim() || 'Votre nom'}
                </button>
              ))}
            </div>
          </>
        )}

        {tab === 'image' && (
          <>
            <p className="small">Choisissez une photo ou un scan de votre signature.</p>
            <label className="sign-row small">
              <input type="checkbox" checked={clearBg} onChange={(e) => setClearBg(e.target.checked)} />
              Rendre le fond blanc transparent
            </label>
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp,image/bmp"
              onChange={async (e) => {
                const file = e.target.files?.[0]
                if (!file) return
                const img = await fromFile(file, clearBg)
                if (keep) remember(img.url)
                onPick(img)
              }}
            />
          </>
        )}

        <label className="sign-row small">
          <input type="checkbox" checked={keep} onChange={(e) => setKeep(e.target.checked)} />
          Garder cette signature sur cet ordinateur
        </label>
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          {tab !== 'image' && (
            <button className="primary" disabled={tab === 'draw' ? empty : !name.trim()} onClick={confirm}>
              Placer sur la page
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
