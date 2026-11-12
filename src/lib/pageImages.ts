import { api } from './api'

// Small cache of rendered pages, keyed by doc/page/width/version.
const cache = new Map<string, Promise<string>>()
const MAX = 120

export function pageImage(docId: string, version: number, page: number, width: number) {
  const w = Math.max(32, Math.round(width / 50) * 50) // bucket widths to reuse renders
  const key = `${docId}:${version}:${page}:${w}`
  let p = cache.get(key)
  if (!p) {
    p = api.renderPage(docId, page, w).then((buf) =>
      URL.createObjectURL(new Blob([buf], { type: 'image/png' })),
    )
    p.catch(() => cache.delete(key))
    cache.set(key, p)
    if (cache.size > MAX) {
      const oldest = cache.keys().next().value!
      cache.get(oldest)?.then((u) => URL.revokeObjectURL(u))
      cache.delete(oldest)
    }
  }
  return p
}
