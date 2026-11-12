import { useState } from 'react'
import licence from '../../LICENSE.md?raw'
import notices from '../../THIRD_PARTY_NOTICES.md?raw'

export const LICENCE_VERSION = '1.0'
const KEY = 'kb.licence.accepted'
const MAIL = 'kevin.barzic@gmail.com'

export function licenceAccepted(): boolean {
  try {
    return localStorage.getItem(KEY) === LICENCE_VERSION
  } catch {
    return false
  }
}

const REQUEST = `mailto:${MAIL}?subject=${encodeURIComponent('Demande de licence professionnelle KB PDF Reader')}&body=${encodeURIComponent(
  [
    'Bonjour,',
    '',
    "Nous souhaitons utiliser KB PDF Reader dans un cadre professionnel.",
    '',
    'Société (dénomination et n° d’immatriculation) : ',
    'Adresse du siège et des sites d’utilisation : ',
    'Nombre de postes : ',
    'Chiffre d’affaires annuel du dernier exercice : ',
    'Personne de contact (nom, fonction, téléphone) : ',
    '',
    'Cordialement,',
  ].join('\n'),
)}`

async function openMail(e: React.MouseEvent) {
  e.preventDefault()
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    await openUrl(REQUEST)
  } catch {
    window.location.href = REQUEST
  }
}

/** Minimal Markdown display (headings, bold, lists, tables as plain lines). */
function Doc({ text }: { text: string }) {
  return (
    <div className="licence-text">
      {text.split('\n').map((line, i) => {
        const bold = (s: string) =>
          s.split(/(\*\*[^*]+\*\*)/).map((p, k) => (p.startsWith('**') ? <strong key={k}>{p.slice(2, -2)}</strong> : p))
        if (line.startsWith('# ')) return <h3 key={i}>{line.slice(2)}</h3>
        if (line.startsWith('## ')) return <h4 key={i}>{line.slice(3)}</h4>
        if (line.startsWith('```') || /^\|-/.test(line)) return null
        if (!line.trim()) return <div key={i} className="gap" />
        return <p key={i}>{bold(line.replace(/^\|\s*|\s*\|$/g, '').replace(/\s*\|\s*/g, ' · '))}</p>
      })}
    </div>
  )
}

interface Props {
  /** First launch: the licence must be accepted to continue. */
  mustAccept: boolean
  onAccept: () => void
  onClose: () => void
}

export function LicenseDialog({ mustAccept, onAccept, onClose }: Props) {
  const [tab, setTab] = useState<'licence' | 'tiers'>('licence')
  const [read, setRead] = useState(false)

  const accept = () => {
    try {
      localStorage.setItem(KEY, LICENCE_VERSION)
    } catch {
      /* ignore */
    }
    onAccept()
  }

  const quit = async () => {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    getCurrentWindow().close()
  }

  return (
    <div className="overlay" onMouseDown={mustAccept ? undefined : onClose}>
      <div className="dialog licence" role="dialog" aria-label="Licence" onMouseDown={(e) => e.stopPropagation()}>
        <h2>{mustAccept ? 'Bienvenue dans KB PDF Reader' : 'À propos de KB PDF Reader'}</h2>
        <div className="licence-summary">
          <p>
            <strong>Usage personnel : gratuit.</strong> Pour une société, un indépendant ou toute utilisation
            professionnelle, une <strong>autorisation écrite</strong> est nécessaire, à demander chaque année à{' '}
            <a href={REQUEST} onClick={openMail}>{MAIL}</a> (nom de la société, adresse, nombre de postes, chiffre d'affaires).
          </p>
          <a className="button-link" href={REQUEST} onClick={openMail}>Demander une licence professionnelle</a>
        </div>
        <div className="segmented">
          <button className={tab === 'licence' ? 'on' : ''} onClick={() => setTab('licence')}>Licence</button>
          <button className={tab === 'tiers' ? 'on' : ''} onClick={() => setTab('tiers')}>Composants tiers</button>
        </div>
        <Doc text={tab === 'licence' ? licence : notices} />
        <p className="muted small">© 2026 Kevin Barzic. Tous droits réservés. Licence version {LICENCE_VERSION}.</p>
        <div className="dialog-actions">
          {mustAccept ? (
            <>
              <label className="small accept">
                <input type="checkbox" checked={read} onChange={(e) => setRead(e.target.checked)} />
                J'ai lu la licence et je l'accepte
              </label>
              <button className="ghost" onClick={quit}>Quitter</button>
              <button className="primary" disabled={!read} onClick={accept}>Accepter</button>
            </>
          ) : (
            <button className="primary" onClick={onClose}>Fermer</button>
          )}
        </div>
      </div>
    </div>
  )
}
