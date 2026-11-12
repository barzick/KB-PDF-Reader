import { useEffect, useRef, useState } from 'react'
import type { SearchHit } from '../lib/api'
import { getPageText, lineRects } from '../lib/pageText'
import { sentences, sortVoices, type Sentence } from '../lib/speech'

const RATES = [0.75, 1, 1.25, 1.5, 2]

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key)
    return v === null ? fallback : (JSON.parse(v) as T)
  } catch {
    return fallback
  }
}

function store(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v))
  } catch {
    /* not kept: fine */
  }
}

/**
 * Reads the document aloud with the system voices, from `start` (page and
 * character), highlighting the sentence being read and following the pages.
 */
export function ReadAloud({
  docId,
  version,
  pageCount,
  start,
  onSentence,
  onClose,
}: {
  docId: string
  version: number
  pageCount: number
  start: { page: number; char: number }
  /** The sentence being read (null when stopped), to highlight and scroll to. */
  onSentence: (hit: SearchHit | null) => void
  onClose: () => void
}) {
  const synth = typeof window !== 'undefined' ? window.speechSynthesis : undefined
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>([])
  const [voiceName, setVoiceName] = useState<string>(() => load('kb.tts.voice', ''))
  const [rate, setRate] = useState<number>(() => load('kb.tts.rate', 1))
  const [state, setState] = useState<'playing' | 'paused' | 'stopped'>('stopped')
  const [where, setWhere] = useState<{ page: number; index: number } | null>(null)
  const [note, setNote] = useState('')
  // Bumped on every jump or stop: callbacks of older utterances are ignored.
  const run = useRef(0)
  const list = useRef<{ page: number; items: Sentence[]; boxes: [number, number, number, number][] } | null>(null)

  useEffect(() => {
    if (!synth) return
    const update = () => setVoices(sortVoices(synth.getVoices()))
    update()
    synth.addEventListener('voiceschanged', update)
    return () => synth.removeEventListener('voiceschanged', update)
  }, [synth])

  const voice = voices.find((v) => v.name === voiceName) ?? voices[0]
  // Read at speaking time, so a change applies from the next sentence.
  const prefs = useRef({ voiceName, rate })
  prefs.current = { voiceName, rate }
  const currentVoice = () => {
    const all = sortVoices(synth?.getVoices() ?? [])
    return all.find((v) => v.name === prefs.current.voiceName) ?? all[0]
  }

  // Stop speaking when the bar closes or the document changes.
  useEffect(
    () => () => {
      run.current++
      synth?.cancel()
      onSentence(null)
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [docId, version],
  )

  /** Sentences of `page` (cached for the page being read). */
  const pageSentences = async (page: number) => {
    if (list.current?.page === page) return list.current
    const t = await getPageText(docId, version, page)
    list.current = { page, items: sentences(t.text), boxes: t.boxes }
    return list.current
  }

  /** Speaks from sentence `index` of `page` (the first one at or after `char` when index is null). */
  const speak = async (page: number, index: number | null, char = 0, token = ++run.current) => {
    if (!synth) return
    // Skip pages without text (blank or scanned without OCR).
    for (let p = page, tries = 0; p < pageCount && tries < 50; p++, tries++) {
      const s = await pageSentences(p)
      if (token !== run.current) return
      const i = index ?? s.items.findIndex((x) => x.end > char)
      if (i >= 0 && i < s.items.length) {
        if (p !== page) setNote('')
        const sentence = s.items[i]
        setWhere({ page: p, index: i })
        setState('playing')
        onSentence({ page: p, rects: lineRects(s.boxes.slice(sentence.start, sentence.end)), context: sentence.text })
        const u = new SpeechSynthesisUtterance(sentence.text)
        const v = currentVoice()
        if (v) {
          u.voice = v
          u.lang = v.lang
        } else u.lang = 'fr-FR'
        u.rate = prefs.current.rate
        u.onend = () => {
          if (token === run.current) speak(p, i + 1, 0, token)
        }
        u.onerror = (e) => {
          if (token !== run.current || e.error === 'interrupted' || e.error === 'canceled') return
          setNote(`Lecture impossible : ${e.error}`)
          setState('stopped')
        }
        synth.cancel()
        synth.speak(u)
        return
      }
      if (i >= s.items.length || s.items.length === 0) {
        index = 0
        char = 0
        if (s.items.length === 0 && p === page) setNote('Pas de texte sur cette page : passage à la suivante.')
        continue
      }
    }
    // End of the document.
    setState('stopped')
    setWhere(null)
    onSentence(null)
  }

  // Start right away.
  useEffect(() => {
    if (synth) speak(start.page, null, start.char)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const step = async (dir: 1 | -1) => {
    if (!where) return
    if (dir === 1) return speak(where.page, where.index + 1)
    if (where.index > 0) return speak(where.page, where.index - 1)
    // Last sentence of the previous page with text.
    for (let p = where.page - 1; p >= 0; p--) {
      const s = await pageSentences(p)
      if (s.items.length) return speak(p, s.items.length - 1)
    }
    speak(where.page, 0)
  }

  const toggle = () => {
    if (!synth) return
    if (state === 'playing') {
      synth.pause()
      setState('paused')
    } else if (state === 'paused') {
      synth.resume()
      setState('playing')
    } else speak(where?.page ?? start.page, where ? where.index : null, start.char)
  }

  const stop = () => {
    run.current++
    synth?.cancel()
    onSentence(null)
    onClose()
  }

  // Voice and speed apply from the current sentence.
  const restart = () => {
    if (where && state === 'playing') speak(where.page, where.index)
  }
  useEffect(restart, [voiceName, rate]) // eslint-disable-line react-hooks/exhaustive-deps

  if (!synth) {
    return (
      <div className="read-bar">
        <span className="small">La lecture à voix haute n'est pas disponible sur cet ordinateur.</span>
        <button className="ghost" onClick={onClose}>Fermer</button>
      </div>
    )
  }

  return (
    <div className="read-bar" role="toolbar" aria-label="Lecture à voix haute">
      <button className="ghost" title="Phrase précédente" onClick={() => step(-1)} disabled={!where}>⏮</button>
      <button className="primary" title={state === 'playing' ? 'Pause' : 'Lire'} onClick={toggle}>
        {state === 'playing' ? '⏸' : '▶'}
      </button>
      <button className="ghost" title="Phrase suivante" onClick={() => step(1)} disabled={!where}>⏭</button>
      <span className="small muted read-where">{where ? `Page ${where.page + 1}` : ''}</span>
      <select
        aria-label="Voix"
        value={voice?.name ?? ''}
        onChange={(e) => {
          setVoiceName(e.target.value)
          store('kb.tts.voice', e.target.value)
        }}
      >
        {voices.length === 0 && <option value="">Voix du système</option>}
        {voices.map((v) => (
          <option key={v.name} value={v.name}>
            {v.name} ({v.lang})
          </option>
        ))}
      </select>
      <select
        aria-label="Vitesse"
        value={rate}
        onChange={(e) => {
          setRate(Number(e.target.value))
          store('kb.tts.rate', Number(e.target.value))
        }}
      >
        {RATES.map((r) => (
          <option key={r} value={r}>× {String(r).replace('.', ',')}</option>
        ))}
      </select>
      {note && <span className="small muted read-note">{note}</span>}
      <button className="ghost" title="Arrêter" onClick={stop}>⏹ Arrêter</button>
    </div>
  )
}
