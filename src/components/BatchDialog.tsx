import { useEffect, useState } from 'react'
import { api, pickFolder, pickPdfPaths, revealFile, type BatchAction, type BatchResult, type CompressLevel } from '../lib/api'

type Kind = 'compress' | 'word' | 'excel' | 'text' | 'images' | 'ocr' | 'watermark' | 'numbers' | 'protect'

const KINDS: [Kind, string][] = [
  ['compress', 'Compresser'],
  ['word', 'Convertir en Word'],
  ['excel', 'Convertir en Excel'],
  ['text', 'Convertir en texte'],
  ['images', 'Convertir en images'],
  ['ocr', 'Reconnaître le texte (OCR)'],
  ['watermark', 'Ajouter un filigrane'],
  ['numbers', 'Numéroter les pages'],
  ['protect', 'Protéger par mot de passe'],
]

const POSITIONS: [string, string][] = [
  ['bottom-center', 'En bas, au centre'],
  ['bottom-right', 'En bas, à droite'],
  ['top-right', 'En haut, à droite'],
]

const name = (p: string) => p.split(/[\\/]/).pop() ?? p

/** One action applied to many PDF files at once. */
export function BatchDialog({ initial, ocrAvailable, onClose }: { initial: string[]; ocrAvailable: boolean; onClose: () => void }) {
  const [files, setFiles] = useState<string[]>(initial)
  const [kind, setKind] = useState<Kind>('compress')
  const [level, setLevel] = useState<CompressLevel>('medium')
  const [format, setFormat] = useState<'png' | 'jpeg'>('png')
  const [dpi, setDpi] = useState(150)
  const [wmText, setWmText] = useState('CONFIDENTIEL')
  const [position, setPosition] = useState('bottom-center')
  const [numFormat, setNumFormat] = useState('Page {page} / {pages}')
  const [pw, setPw] = useState('')
  const [pw2, setPw2] = useState('')
  const [running, setRunning] = useState<string | null>(null)
  const [results, setResults] = useState<BatchResult[] | null>(null)
  const [error, setError] = useState('')

  useEffect(() => {
    let unlisten: (() => void) | undefined
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen<{ done: number; total: number; file: string }>('batch-progress', (e) =>
        setRunning(`Fichier ${e.payload.done + 1}/${e.payload.total} : ${e.payload.file}`),
      ).then((u) => (unlisten = u)),
    )
    return () => unlisten?.()
  }, [])

  const add = async () => {
    const more = await pickPdfPaths()
    setFiles((f) => [...f, ...more.filter((p) => !f.includes(p))])
    setResults(null)
  }

  const action = (): BatchAction | null => {
    switch (kind) {
      case 'compress':
        return { action: 'compress', level }
      case 'word':
      case 'excel':
      case 'text':
        return { action: 'convert', target: { kind } }
      case 'images':
        return { action: 'convert', target: { kind: 'images', format, dpi } }
      case 'ocr':
        return { action: 'ocr', languages: [] }
      case 'watermark':
        return {
          action: 'watermark',
          watermark: {
            text: wmText,
            style: { family: 'sans', bold: true, italic: false, size: 60, color: [200, 30, 30], opacity: 0.25 },
            angle: 45,
            pages: null,
          },
        }
      case 'numbers': {
        const slots: [string, string, string] = ['', '', '']
        const [where, side] = position.split('-')
        slots[side === 'right' ? 2 : 1] = numFormat
        return {
          action: 'header_footer',
          spec: {
            header: where === 'top' ? slots : ['', '', ''],
            footer: where === 'bottom' ? slots : ['', '', ''],
            style: { family: 'sans', bold: false, italic: false, size: 10, color: [0, 0, 0], opacity: 1 },
            margin: 28,
            start: 1,
            pages: null,
            date: new Date().toLocaleDateString('fr-CH'),
            file: '',
          },
        }
      }
      case 'protect':
        return {
          action: 'protect',
          options: { user_password: pw, owner_password: '', allow_print: true, allow_copy: true, allow_modify: false },
        }
    }
  }

  const invalid =
    !files.length ||
    (kind === 'watermark' && !wmText.trim()) ||
    (kind === 'numbers' && !numFormat.includes('{page}')) ||
    (kind === 'protect' && (pw.length < 4 || pw !== pw2)) ||
    (kind === 'ocr' && !ocrAvailable)

  const start = async () => {
    const a = action()
    if (!a) return
    const dir = await pickFolder()
    if (!dir) return
    setError('')
    setResults(null)
    setRunning('Préparation…')
    try {
      setResults(await api.batchProcess(files, a, dir))
    } catch (e) {
      setError(String(e))
    } finally {
      setRunning(null)
    }
  }

  const ok = results?.filter((r) => !r.error) ?? []
  const firstOutput = ok.find((r) => r.outputs.length)?.outputs[0]

  return (
    <div className="overlay" onMouseDown={running ? undefined : onClose}>
      <div className="dialog wide" role="dialog" aria-label="Traitement par lots" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Traitement par lots</h2>
        <div className="batch-files">
          {files.length === 0 && <p className="small muted pad">Ajoutez les PDF à traiter. Les originaux ne sont pas modifiés.</p>}
          {files.map((f) => {
            const r = results?.find((x) => x.file === name(f))
            return (
              <div key={f} className="batch-file" title={f}>
                <span className={r ? (r.error ? 'danger-text' : 'ok-text') : ''}>{r ? (r.error ? '✕' : '✓') : '•'}</span>
                <span className="batch-name">{name(f)}</span>
                {r?.error && <span className="small danger-text">{r.error}</span>}
                {!running && (
                  <button className="ghost small" title="Retirer" onClick={() => setFiles(files.filter((x) => x !== f))}>✕</button>
                )}
              </div>
            )
          })}
        </div>
        <div className="row-start">
          <button onClick={add} disabled={!!running}>+ Ajouter des PDF…</button>
          {files.length > 0 && <span className="small muted">{files.length} fichier(s)</span>}
        </div>
        <label className="field">
          Action
          <select value={kind} onChange={(e) => (setKind(e.target.value as Kind), setResults(null))}>
            {KINDS.map(([k, l]) => (
              <option key={k} value={k} disabled={k === 'ocr' && !ocrAvailable}>{l}</option>
            ))}
          </select>
        </label>
        {kind === 'compress' && (
          <label className="field">
            Niveau
            <select value={level} onChange={(e) => setLevel(e.target.value as CompressLevel)}>
              <option value="lossless">Sans perte</option>
              <option value="light">Léger</option>
              <option value="medium">Moyen (recommandé)</option>
              <option value="strong">Fort</option>
            </select>
          </label>
        )}
        {kind === 'images' && (
          <div className="field-row">
            <label className="field">
              Format
              <select value={format} onChange={(e) => setFormat(e.target.value as 'png' | 'jpeg')}>
                <option value="png">PNG</option>
                <option value="jpeg">JPEG</option>
              </select>
            </label>
            <label className="field">
              Résolution
              <select value={dpi} onChange={(e) => setDpi(Number(e.target.value))}>
                <option value={72}>72 ppp</option>
                <option value={150}>150 ppp</option>
                <option value={300}>300 ppp</option>
              </select>
            </label>
          </div>
        )}
        {kind === 'watermark' && (
          <label className="field">
            Texte du filigrane
            <input value={wmText} onChange={(e) => setWmText(e.target.value)} />
          </label>
        )}
        {kind === 'numbers' && (
          <div className="field-row">
            <label className="field">
              Position
              <select value={position} onChange={(e) => setPosition(e.target.value)}>
                {POSITIONS.map(([v, l]) => (
                  <option key={v} value={v}>{l}</option>
                ))}
              </select>
            </label>
            <label className="field">
              Texte
              <input value={numFormat} onChange={(e) => setNumFormat(e.target.value)} />
            </label>
          </div>
        )}
        {kind === 'protect' && (
          <div className="field-row">
            <label className="field">
              Mot de passe
              <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} />
            </label>
            <label className="field">
              Confirmation
              <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
            </label>
          </div>
        )}
        {kind === 'protect' && pw2 && pw !== pw2 && <span className="small danger-text">Les mots de passe diffèrent.</span>}
        {running && <p className="small">{running}</p>}
        {error && <p className="small danger-text">{error}</p>}
        {results && (
          <p className="small">
            {ok.length}/{results.length} fichier(s) traité(s).{' '}
            {firstOutput && (
              <button className="link" onClick={() => revealFile(firstOutput).catch(() => {})}>Afficher le dossier</button>
            )}
          </p>
        )}
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose} disabled={!!running}>{results ? 'Fermer' : 'Annuler'}</button>
          <button className="primary" disabled={invalid || !!running} onClick={start}>Choisir le dossier et lancer…</button>
        </div>
      </div>
    </div>
  )
}
