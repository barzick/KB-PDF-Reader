import { useRef, useState } from 'react'
import type { Rect } from '../lib/api'
import type { PlacedImage } from './SignatureDialog'

interface Props {
  image: PlacedImage
  page: number
  pageHeight: number
  scale: number
  /** Default width in points for a simple click. */
  width: number
  onPlace: (page: number, rect: Rect) => void
}

/** Click (or drag a box) to put an image on the page. */
export function PlaceLayer({ image, page, pageHeight, scale, width, onPlace }: Props) {
  const el = useRef<HTMLDivElement>(null)
  const [hover, setHover] = useState<[number, number] | null>(null)
  const [drag, setDrag] = useState<{ from: [number, number]; to: [number, number] } | null>(null)
  const ratio = image.height / image.width

  const at = (e: React.MouseEvent): [number, number] => {
    const r = el.current!.getBoundingClientRect()
    return [e.clientX - r.left, e.clientY - r.top]
  }
  /** Screen box → PDF rect (keeping the image's proportions inside the box). */
  const toRect = (x: number, y: number, w: number, h: number): Rect => {
    const fit = Math.min(w, h / ratio)
    const [iw, ih] = [fit, fit * ratio]
    const [cx, cy] = [x + (w - iw) / 2, y + (h - ih) / 2]
    return [cx / scale, pageHeight - (cy + ih) / scale, (cx + iw) / scale, pageHeight - cy / scale]
  }
  const dw = width * scale
  const dh = dw * ratio

  let ghost: React.CSSProperties | null = null
  if (drag) {
    const [x0, y0] = drag.from
    const [x1, y1] = drag.to
    const r = toRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0))
    ghost = { left: r[0] * scale, top: (pageHeight - r[3]) * scale, width: (r[2] - r[0]) * scale, height: (r[3] - r[1]) * scale }
  } else if (hover) {
    ghost = { left: hover[0] - dw / 2, top: hover[1] - dh / 2, width: dw, height: dh }
  }

  return (
    <div
      ref={el}
      className="place-layer"
      onMouseMove={(e) => {
        const p = at(e)
        if (drag) setDrag({ ...drag, to: p })
        else setHover(p)
      }}
      onMouseLeave={() => setHover(null)}
      onMouseDown={(e) => {
        if (e.button !== 0) return
        e.preventDefault()
        const p = at(e)
        setDrag({ from: p, to: p })
      }}
      onMouseUp={() => {
        if (!drag) return
        const [x0, y0] = drag.from
        const [x1, y1] = drag.to
        setDrag(null)
        if (Math.abs(x1 - x0) > 12 && Math.abs(y1 - y0) > 8) {
          onPlace(page, toRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0)))
        } else {
          onPlace(page, toRect(x0 - dw / 2, y0 - dh / 2, dw, dh))
        }
      }}
    >
      {ghost && <img className="place-ghost" src={image.url} style={ghost} alt="" draggable={false} />}
    </div>
  )
}
