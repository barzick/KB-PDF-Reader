import { useEffect, useRef, useState } from 'react'

export interface MenuItem {
  label: string
  onClick: () => void
  disabled?: boolean
  danger?: boolean
  hint?: string
}

/** A toolbar button opening a list of actions ("—" separators allowed). */
export function Menu({ label, items, disabled }: { label: string; items: (MenuItem | '—')[]; disabled?: boolean }) {
  const [open, setOpen] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const close = (e: MouseEvent) => !root.current?.contains(e.target as Node) && setOpen(false)
    const esc = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', esc)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', esc)
    }
  }, [open])
  return (
    <div className="menu" ref={root}>
      <button className={open ? 'on' : ''} disabled={disabled} onClick={() => setOpen((v) => !v)}>
        {label} ▾
      </button>
      {open && (
        <div className="menu-list" role="menu">
          {items.map((it, i) =>
            it === '—' ? (
              <hr key={i} />
            ) : (
              <button
                key={i}
                role="menuitem"
                className={it.danger ? 'danger' : ''}
                disabled={it.disabled}
                onClick={() => {
                  setOpen(false)
                  it.onClick()
                }}
              >
                <span>{it.label}</span>
                {it.hint && <span className="muted small">{it.hint}</span>}
              </button>
            ),
          )}
        </div>
      )}
    </div>
  )
}
