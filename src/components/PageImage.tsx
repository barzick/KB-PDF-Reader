import { useEffect, useRef, useState } from 'react'
import { pageImage } from '../lib/pageImages'

interface Props {
  docId: string
  version: number
  page: number
  /** Display size in CSS pixels. */
  width: number
  height: number
  className?: string
}

/** A PDF page rendered by PDFium, loaded only when it comes near the screen. */
export function PageImage({ docId, version, page, width, height, className }: Props) {
  const box = useRef<HTMLDivElement>(null)
  const [visible, setVisible] = useState(false)
  const [src, setSrc] = useState<string | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    const el = box.current
    if (!el) return
    const io = new IntersectionObserver(([e]) => setVisible(e.isIntersecting), { rootMargin: '800px' })
    io.observe(el)
    return () => io.disconnect()
  }, [])

  useEffect(() => {
    if (!visible) return
    let live = true
    const px = width * (window.devicePixelRatio || 1)
    pageImage(docId, version, page, px)
      .then((u) => live && (setSrc(u), setError(false)))
      .catch(() => live && setError(true))
    return () => {
      live = false
    }
  }, [visible, docId, version, page, width])

  return (
    <div ref={box} className={`page ${className ?? ''}`} style={{ width, height }}>
      {src && <img src={src} alt={`Page ${page + 1}`} draggable={false} />}
      {error && <span className="page-error">Impossible d'afficher cette page</span>}
    </div>
  )
}
