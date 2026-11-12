/** A sentence of a page's text: code point range [start, end) and the text to speak. */
export interface Sentence {
  start: number
  end: number
  text: string
}

const END = /[.!?…]/
const CONTINUES = /[\p{Ll},\-–(«"']/u

/**
 * Cuts page text into sentences: after . ! ? … followed by a space, and at
 * line breaks that do not continue a sentence (titles, addresses, list items).
 */
export function sentences(text: string): Sentence[] {
  const chars = [...text]
  const out: Sentence[] = []
  let start = 0
  const push = (end: number) => {
    const t = chars.slice(start, end).join('').replace(/\s+/g, ' ').trim()
    // Skip page numbers and lone symbols.
    if (/[\p{L}]{2,}|\p{N}{2,}/u.test(t)) out.push({ start, end, text: t })
    start = end
  }
  for (let i = 0; i < chars.length; i++) {
    const c = chars[i]
    const next = chars[i + 1]
    if (END.test(c) && (next === undefined || /\s/.test(next)) && !/^\s*\p{N}/u.test(chars.slice(i + 1, i + 3).join('')) ) {
      push(i + 1)
    } else if (c === '\n') {
      const prev = chars.slice(start, i).join('').trimEnd().slice(-1)
      const next = chars.slice(i + 1).join('').trimStart().charAt(0)
      // A line ending mid-sentence goes on in lower case; a capital starts a new item (table row, list).
      if (prev && (!CONTINUES.test(prev) || (/\p{Ll}/u.test(prev) && /\p{Lu}/u.test(next)))) push(i + 1)
    }
  }
  if (start < chars.length) push(chars.length)
  return out
}

/** Voices for French first (Swiss, then France), then the others. */
export function sortVoices(voices: SpeechSynthesisVoice[]): SpeechSynthesisVoice[] {
  const rank = (v: SpeechSynthesisVoice) => (v.lang === 'fr-CH' ? 0 : v.lang === 'fr-FR' ? 1 : v.lang.startsWith('fr') ? 2 : 3)
  return [...voices].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
}
