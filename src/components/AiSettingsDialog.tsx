import { useEffect, useState } from 'react'
import { api, type AiProfile, type AiSettings } from '../lib/api'

interface Preset {
  label: string
  kind: AiProfile['kind']
  endpoint: string
  model: string
  key: 'required' | 'none'
  hint: string
}

const PRESETS: Preset[] = [
  { label: 'OpenAI', kind: 'openai', endpoint: 'https://api.openai.com/v1', model: '', key: 'required', hint: 'Clé sur platform.openai.com.' },
  { label: 'Anthropic (Claude)', kind: 'anthropic', endpoint: 'https://api.anthropic.com', model: 'claude-opus-5-5', key: 'required', hint: 'Clé sur platform.claude.com.' },
  { label: 'Mistral', kind: 'openai', endpoint: 'https://api.mistral.ai/v1', model: '', key: 'required', hint: 'Clé sur console.mistral.ai.' },
  { label: 'Google Gemini', kind: 'openai', endpoint: 'https://generativelanguage.googleapis.com/v1beta/openai', model: '', key: 'required', hint: 'Clé sur aistudio.google.com.' },
  { label: 'OpenRouter', kind: 'openai', endpoint: 'https://openrouter.ai/api/v1', model: '', key: 'required', hint: 'Un compte, de nombreux modèles.' },
  { label: 'Ollama (sur cet ordinateur)', kind: 'openai', endpoint: 'http://localhost:11434/v1', model: '', key: 'none', hint: 'Rien ne quitte l’ordinateur. Ollama doit être lancé.' },
  { label: 'LM Studio (sur cet ordinateur)', kind: 'openai', endpoint: 'http://localhost:1234/v1', model: '', key: 'none', hint: 'Rien ne quitte l’ordinateur. Activez le serveur local de LM Studio.' },
  { label: 'Autre service compatible OpenAI', kind: 'openai', endpoint: '', model: '', key: 'required', hint: 'Adresse se terminant en général par /v1.' },
]

const CONTEXTS: [number, string][] = [
  [8_000, 'Très court (8 000 caractères)'],
  [30_000, 'Court (30 000)'],
  [60_000, 'Normal (60 000)'],
  [150_000, 'Long (150 000)'],
  [400_000, 'Très long (400 000)'],
]

const blank = (): AiProfile => ({
  id: `p${Date.now().toString(36)}`,
  name: '',
  kind: 'openai',
  endpoint: '',
  model: '',
  context_chars: 60_000,
})

/** Where the user plugs in their own model: service, address, key, model. */
export function AiSettingsDialog({ onClose, onChange }: { onClose: () => void; onChange: (s: AiSettings) => void }) {
  const [settings, setSettings] = useState<AiSettings | null>(null)
  const [edit, setEdit] = useState<AiProfile | null>(null)
  const [key, setKey] = useState('')
  const [models, setModels] = useState<string[]>([])
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null)
  const [busy, setBusy] = useState(false)

  const apply = (s: AiSettings) => {
    setSettings(s)
    onChange(s)
  }

  useEffect(() => {
    api.aiSettings().then((s) => {
      setSettings(s)
      const a = s.profiles.find((p) => p.id === s.active) ?? s.profiles[0]
      setEdit(a ? { ...a } : blank())
    })
  }, [])

  const stored = settings?.profiles.find((p) => p.id === edit?.id)
  const isNew = !stored
  const local = !!edit && /localhost|127\.0\.0\.1/.test(edit.endpoint)

  const pick = (p: (typeof settings & {})['profiles'][number] | null) => {
    setEdit(p ? { ...p } : null)
    setKey('')
    setModels([])
    setStatus(null)
  }

  const usePreset = (label: string) => {
    const pr = PRESETS.find((p) => p.label === label)
    if (!pr || !edit) return
    setEdit({ ...edit, kind: pr.kind, endpoint: pr.endpoint, model: pr.model || edit.model, name: edit.name || pr.label.replace(/ \(.*\)$/, '') })
    setModels([])
    setStatus(null)
  }

  const preset = PRESETS.find((p) => p.endpoint && p.endpoint === edit?.endpoint && p.kind === edit?.kind)

  const save = async (): Promise<AiSettings | null> => {
    if (!edit) return null
    const p = { ...edit, name: edit.name.trim() || 'Mon modèle', endpoint: edit.endpoint.trim(), model: edit.model.trim() }
    const s = await api.aiSaveProfile(p, key ? key : null)
    apply(s)
    setEdit(p)
    setKey('')
    return s
  }

  const test = async () => {
    if (!edit) return
    setBusy(true)
    setStatus(null)
    try {
      await save()
      const list = await api.aiListModels(edit.id)
      setModels(list)
      if (edit.model.trim() && list.length && !list.includes(edit.model.trim()))
        setStatus({ ok: false, text: `Connexion réussie, mais le modèle « ${edit.model} » n'est pas dans la liste du service.` })
      else setStatus({ ok: true, text: `Connexion réussie : ${list.length} modèle(s) disponible(s).` })
      if (!edit.model.trim() && list.length) setEdit((e) => (e ? { ...e, model: list[0] } : e))
    } catch (e) {
      setStatus({ ok: false, text: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dialog wide" role="dialog" aria-label="Réglages de l'assistant" onMouseDown={(e) => e.stopPropagation()}>
        <h2>Assistant IA : votre modèle</h2>
        <p className="small muted">
          Branchez le service d'IA de votre choix avec votre propre clé, ou un modèle installé sur cet ordinateur (Ollama, LM Studio).
          KB PDF Reader n'inclut aucun service et ne voit pas vos échanges.
        </p>
        <div className="ai-profiles">
          {settings?.profiles.map((p) => (
            <button key={p.id} className={edit?.id === p.id ? 'on' : ''} onClick={() => pick(p)}>
              {p.id === settings.active ? '● ' : ''}
              {p.name}
            </button>
          ))}
          <button className="ghost" onClick={() => (pick(null), setEdit(blank()))}>+ Ajouter</button>
        </div>
        {edit && (
          <>
            <label className="field">
              Service
              <select value={preset?.label ?? ''} onChange={(e) => usePreset(e.target.value)}>
                {!preset && <option value="">Personnalisé</option>}
                {PRESETS.map((p) => (
                  <option key={p.label} value={p.label}>{p.label}</option>
                ))}
              </select>
              {preset && <span className="small muted">{preset.hint}</span>}
            </label>
            <div className="field-row">
              <label className="field">
                Nom
                <input value={edit.name} placeholder="Mon modèle" onChange={(e) => setEdit({ ...edit, name: e.target.value })} />
              </label>
              <label className="field">
                Type d'interface
                <select value={edit.kind} onChange={(e) => setEdit({ ...edit, kind: e.target.value as AiProfile['kind'] })}>
                  <option value="openai">Compatible OpenAI</option>
                  <option value="anthropic">Anthropic</option>
                </select>
              </label>
            </div>
            <label className="field">
              Adresse
              <input value={edit.endpoint} placeholder="https://…/v1" onChange={(e) => setEdit({ ...edit, endpoint: e.target.value })} />
            </label>
            <label className="field">
              Clé API {local && <span className="small muted">(inutile en local)</span>}
              <input
                type="password"
                value={key}
                autoComplete="off"
                placeholder={stored?.has_key ? 'Clé enregistrée : laisser vide pour la garder' : local ? 'Aucune' : 'Collez votre clé'}
                onChange={(e) => setKey(e.target.value)}
              />
            </label>
            <div className="field-row">
              <label className="field">
                Modèle
                <input list="ai-models" value={edit.model} placeholder="nom du modèle" onChange={(e) => setEdit({ ...edit, model: e.target.value })} />
                <datalist id="ai-models">
                  {models.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
              </label>
              <label className="field">
                Texte du document envoyé
                <select value={edit.context_chars} onChange={(e) => setEdit({ ...edit, context_chars: Number(e.target.value) })}>
                  {CONTEXTS.map(([v, l]) => (
                    <option key={v} value={v}>{l}</option>
                  ))}
                </select>
              </label>
            </div>
            <p className="small muted">
              {local
                ? 'Le texte du document reste sur cet ordinateur.'
                : 'Le texte des pages utiles est envoyé au service choisi avec chaque question.'}{' '}
              La clé est enregistrée sur cet ordinateur seulement.
            </p>
            {status && <p className={`small ${status.ok ? 'ok-text' : 'danger-text'}`}>{status.text}</p>}
          </>
        )}
        <div className="dialog-actions">
          {edit && !isNew && (
            <button
              className="ghost danger-text"
              onClick={async () => {
                const s = await api.aiDeleteProfile(edit.id)
                apply(s)
                pick(s.profiles[0] ?? null)
              }}
            >
              Supprimer
            </button>
          )}
          <span className="grow" />
          {edit && <button className="ghost" onClick={onClose}>Annuler</button>}
          {edit && (
            <button onClick={test} disabled={busy || !edit.endpoint.trim()}>
              {busy ? 'Test…' : 'Tester et lister les modèles'}
            </button>
          )}
          {edit && (
            <button
              className="primary"
              disabled={!edit.endpoint.trim() || !edit.model.trim()}
              onClick={async () => {
                const s = await save()
                if (s) apply(await api.aiSetActive(edit.id))
                onClose()
              }}
            >
              Enregistrer et utiliser
            </button>
          )}
          {!edit && <button className="ghost" onClick={onClose}>Fermer</button>}
        </div>
      </div>
    </div>
  )
}
