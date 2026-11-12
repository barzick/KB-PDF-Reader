import { api, type PageText } from './api'

// Text of pages, cached per document version.
const cache = new Map<string, Promise<PageText>>()

export function getPageText(docId: string, version: number, page: number): Promise<PageText> {
  const key = `${docId}:${version}:${page}`
  let p = cache.get(key)
  if (!p) {
    p = api.pageText(docId, page).catch(() => ({ text: '', boxes: [] }))
    cache.set(key, p)
    if (cache.size > 400) cache.delete(cache.keys().next().value!)
  }
  return p
}

type Box = [number, number, number, number]

/** One rectangle per line for a run of character boxes. */
export function lineRects(boxes: Box[]): Box[] {
  const out: Box[] = []
  for (const b of boxes) {
    if (b[2] <= b[0]) continue
    const r = out[out.length - 1]
    if (r && Math.abs(b[1] - r[1]) < 0.5 * Math.max(r[3] - r[1], 1) && b[0] >= r[0] - 2) {
      r[0] = Math.min(r[0], b[0])
      r[1] = Math.min(r[1], b[1])
      r[2] = Math.max(r[2], b[2])
      r[3] = Math.max(r[3], b[3])
    } else out.push([...b] as Box)
  }
  return out
}

/** Index of the character under (or nearest to) a point in PDF coordinates. */
export function charAt(t: PageText, x: number, y: number): number | null {
  let best: number | null = null
  let bestD = Infinity
  t.boxes.forEach((b, i) => {
    if (b[2] <= b[0]) return
    const dx = x < b[0] ? b[0] - x : x > b[2] ? x - b[2] : 0
    const dy = y < b[1] ? b[1] - y : y > b[3] ? y - b[3] : 0
    // Being on the same line matters more than horizontal distance.
    const d = dx + dy * 4
    if (d < bestD) {
      bestD = d
      best = i
    }
  })
  return best
}

const WORD = /[\p{L}\p{N}_'’-]/u

export function wordAround(t: PageText, i: number): [number, number] {
  const chars = [...t.text]
  let a = i
  let b = i
  while (a > 0 && WORD.test(chars[a - 1])) a--
  while (b < chars.length - 1 && WORD.test(chars[b + 1])) b++
  return [a, b]
}

export function sliceText(t: PageText, a: number, b: number): string {
  return [...t.text].slice(Math.min(a, b), Math.max(a, b) + 1).join('')
}
