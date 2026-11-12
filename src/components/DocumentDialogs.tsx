import { useEffect, useState } from 'react'
import { api, parsePages, type CompressLevel, type Crop, type ProtectOptions, type ProtectionInfo } from '../lib/api'

/** Asks the password of a protected file. */
export function PasswordDialog({
  name,
  error,
  onOpen,
  onSkip,
}: {
  name: string
  error: string | null
  onOpen: (password: string) => void
  onSkip: () => void
}) {
  const [pw, setPw] = useState('')
  return (
    <div className="overlay">
      <form
        className="dialog"
        role="dialog"
        aria-label="Mot de passe"
        onSubmit={(e) => {
          e.preventDefault()
          if (pw) onOpen(pw)
        }}
      >
        <h2>Document protégé</h2>
        <p>
          <strong>{name}</strong> demande un mot de passe pour s'ouvrir.
        </p>
        <input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Mot de passe" />
        {error && <span className="small danger-text">{error}</span>}
        <div className="dialog-actions">
          <button type="button" className="ghost" onClick={onSkip}>Ne pas ouvrir</button>
          <button type="submit" className="primary" disabled={!pw}>Ouvrir</button>
        </div>
      </form>
    </div>
  )
}

/** Add, change or remove the protection applied when saving. */
export function ProtectDialog({
  docId,
  onApply,
  onClose,
}: {
  docId: string
  onApply: (p: ProtectOptions | null) => void
  onClose: () => void
}) {
  const [info, setInfo] = useState<ProtectionInfo | null>(null)
  const [pw, setPw] = useState('')
  const [pw2, setPw2] = useState('')
  const [owner, setOwner] = useState('')
  const [print, setPrint] = useState(true)
  const [copy, setCopy] = useState(true)
  const [modify, setModify] = useState(true)

  useEffect(() => {
    api.docProtection(docId).then((i) => {
      setInfo(i)
      setPrint(i.allow_print)
      setCopy(i.allow_copy)
      setModify(i.allow_modify)
    })
  }, [docId])

  const restricted = !print || !copy || !modify
  const mismatch = pw !== pw2
  const canApply = !mismatch && (pw.length > 0 || restricted)

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Protection" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Protéger par mot de passe</h2>
        {info?.protected && (
          <p className="licence-summary small">
            Ce document est protégé {info.has_password ? 'par un mot de passe' : '(restrictions sans mot de passe)'}. La
            protection sera remise à l'enregistrement.
          </p>
        )}
        <label className="field">
          Mot de passe pour ouvrir
          <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Laisser vide : pas de mot de passe à l'ouverture" />
        </label>
        <label className="field">
          Confirmer
          <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
          {mismatch && pw2 && <span className="small danger-text">Les deux mots de passe sont différents.</span>}
        </label>
        <p className="small muted">Autoriser :</p>
        <label className="sign-row small">
          <input type="checkbox" checked={print} onChange={(e) => setPrint(e.target.checked)} /> Impression
        </label>
        <label className="sign-row small">
          <input type="checkbox" checked={copy} onChange={(e) => setCopy(e.target.checked)} /> Copie du texte
        </label>
        <label className="sign-row small">
          <input type="checkbox" checked={modify} onChange={(e) => setModify(e.target.checked)} /> Modification et annotations
        </label>
        {restricted && (
          <label className="field">
            Mot de passe du propriétaire (pour lever les restrictions plus tard)
            <input type="password" value={owner} onChange={(e) => setOwner(e.target.value)} placeholder="Facultatif" />
          </label>
        )}
        <p className="small muted">Chiffrement AES 256 bits, appliqué quand vous enregistrez. Notez le mot de passe : il ne peut pas être retrouvé.</p>
        <div className="dialog-actions">
          {info?.protected && (
            <button className="ghost danger-text" onClick={() => onApply(null)}>
              Retirer la protection
            </button>
          )}
          <span className="grow" />
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button
            className="primary"
            disabled={!canApply}
            onClick={() =>
              onApply({ user_password: pw, owner_password: owner, allow_print: print, allow_copy: copy, allow_modify: modify })
            }
          >
            Protéger
          </button>
        </div>
      </div>
    </div>
  )
}

export function CompressDialog({ onApply, onClose }: { onApply: (l: CompressLevel) => void; onClose: () => void }) {
  const [level, setLevel] = useState<CompressLevel>('medium')
  const levels: [CompressLevel, string, string][] = [
    ['lossless', 'Sans perte', 'Nettoie et compacte le fichier ; les images restent intactes.'],
    ['light', 'Légère', 'Images en haute qualité (jusqu’à 2600 px).'],
    ['medium', 'Moyenne', 'Bon compromis pour envoyer par e-mail (1800 px).'],
    ['strong', 'Forte', 'Fichier le plus léger ; images moins nettes (1200 px).'],
  ]
  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Compresser" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Compresser</h2>
        {levels.map(([v, label, hint]) => (
          <label key={v} className="choice">
            <input type="radio" name="level" checked={level === v} onChange={() => setLevel(v)} />
            <span>
              <strong>{label}</strong>
              <br />
              <span className="small muted">{hint}</span>
            </span>
          </label>
        ))}
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" onClick={() => onApply(level)}>Compresser</button>
        </div>
      </div>
    </div>
  )
}

export function CropDialog({
  pageCount,
  current,
  onApply,
  onClose,
}: {
  pageCount: number
  current: number
  onApply: (pages: number[] | null, crop: Crop) => void
  onClose: () => void
}) {
  const [mode, setMode] = useState<Crop['mode']>('margins')
  const [mm, setMm] = useState<[number, number, number, number]>([10, 10, 10, 10])
  const [pages, setPages] = useState('')
  const [pad, setPad] = useState(5)
  const toPt = (v: number) => (v * 72) / 25.4

  let error = ''
  try {
    parsePages(pages, pageCount)
  } catch (e) {
    error = (e as Error).message
  }

  const side = (i: number, label: string) => (
    <label className="field">
      {label}
      <input
        type="number"
        min={0}
        value={mm[i]}
        onChange={(e) => {
          const n = [...mm] as typeof mm
          n[i] = Math.max(0, Number(e.target.value))
          setMm(n)
        }}
      />
    </label>
  )

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Rogner" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Rogner les pages</h2>
        <div className="segmented">
          <button className={mode === 'margins' ? 'on' : ''} onClick={() => setMode('margins')}>Marges</button>
          <button className={mode === 'content' ? 'on' : ''} onClick={() => setMode('content')}>Autour du contenu</button>
          <button className={mode === 'reset' ? 'on' : ''} onClick={() => setMode('reset')}>Page entière</button>
        </div>
        {mode === 'margins' && (
          <div className="crop-grid">
            {side(0, 'Haut (mm)')}
            {side(1, 'Droite (mm)')}
            {side(2, 'Bas (mm)')}
            {side(3, 'Gauche (mm)')}
          </div>
        )}
        {mode === 'content' && (
          <label className="field">
            Marge autour du contenu (mm)
            <input type="number" min={0} value={pad} onChange={(e) => setPad(Math.max(0, Number(e.target.value)))} />
          </label>
        )}
        {mode === 'reset' && <p className="small muted">Retire le rognage : la page entière redevient visible.</p>}
        <p className="small muted">Rien n'est effacé : la partie coupée est seulement masquée et peut être rétablie.</p>
        <label className="field">
          Pages
          <input placeholder={`Toutes (1-${pageCount})`} value={pages} onChange={(e) => setPages(e.target.value)} />
          {error ? (
            <span className="small danger-text">{error}</span>
          ) : (
            <button className="link small" onClick={() => setPages(String(current + 1))}>
              Seulement la page {current + 1}
            </button>
          )}
        </label>
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button
            className="primary"
            disabled={!!error}
            onClick={() => {
              const crop: Crop =
                mode === 'margins'
                  ? { mode, margins: mm.map(toPt) as [number, number, number, number] }
                  : mode === 'content'
                    ? { mode, pad: toPt(pad) }
                    : { mode }
              onApply(parsePages(pages, pageCount), crop)
            }}
          >
            Rogner
          </button>
        </div>
      </div>
    </div>
  )
}

export function SplitDialog({
  pageCount,
  onApply,
  onClose,
}: {
  pageCount: number
  onApply: (groups: number[][]) => void
  onClose: () => void
}) {
  const [mode, setMode] = useState<'every' | 'ranges' | 'each'>('every')
  const [n, setN] = useState(Math.min(5, Math.max(1, Math.ceil(pageCount / 2))))
  const [ranges, setRanges] = useState('')

  let groups: number[][] = []
  let error = ''
  if (mode === 'each') groups = Array.from({ length: pageCount }, (_, i) => [i])
  else if (mode === 'every') {
    for (let i = 0; i < pageCount; i += n) groups.push(Array.from({ length: Math.min(n, pageCount - i) }, (_, k) => i + k))
  } else {
    try {
      groups = ranges
        .split(/[;,]/)
        .map((r) => r.trim())
        .filter(Boolean)
        .map((r) => parsePages(r, pageCount) ?? [])
        .filter((g) => g.length)
      if (!groups.length && ranges.trim()) error = 'Aucune page valide'
    } catch (e) {
      error = (e as Error).message
    }
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog" role="dialog" aria-label="Diviser" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Diviser le document</h2>
        <div className="segmented">
          <button className={mode === 'every' ? 'on' : ''} onClick={() => setMode('every')}>Toutes les N pages</button>
          <button className={mode === 'ranges' ? 'on' : ''} onClick={() => setMode('ranges')}>Par plages</button>
          <button className={mode === 'each' ? 'on' : ''} onClick={() => setMode('each')}>Une page par fichier</button>
        </div>
        {mode === 'every' && (
          <label className="field">
            Pages par fichier
            <input type="number" min={1} max={pageCount} value={n} onChange={(e) => setN(Math.max(1, Number(e.target.value) || 1))} />
          </label>
        )}
        {mode === 'ranges' && (
          <label className="field">
            Plages, séparées par des virgules
            <input autoFocus placeholder="1-3, 4-7, 8" value={ranges} onChange={(e) => setRanges(e.target.value)} />
            {error && <span className="small danger-text">{error}</span>}
          </label>
        )}
        <p className="small muted">
          {groups.length} fichier(s) seront créés dans le dossier de votre choix ; le document ouvert ne change pas.
        </p>
        <div className="dialog-actions">
          <button className="ghost" onClick={onClose}>Annuler</button>
          <button className="primary" disabled={!groups.length || !!error} onClick={() => onApply(groups)}>
            Choisir le dossier…
          </button>
        </div>
      </div>
    </div>
  )
}
