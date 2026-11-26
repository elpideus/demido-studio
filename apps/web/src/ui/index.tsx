/* Thin React wrappers around the brand components (brand/css/components). They add no styles of their own:
   the look lives in the brand kit, so the kit page and the app cannot drift apart. */
import { type ButtonHTMLAttributes, type CSSProperties, type ReactNode, useCallback, useEffect, useId, useRef, useState } from 'react'
import { Dialog as BaseDialog } from '@base-ui/react/dialog'
import { Menu as BaseMenu } from '@base-ui/react/menu'
import { ContextMenu as BaseContextMenu } from '@base-ui/react/context-menu'
import { Tooltip as BaseTooltip } from '@base-ui/react/tooltip'
import { Popover as BasePopover } from '@base-ui/react/popover'
import { Info, PanelRight, PanelRightClose } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { memoryBudget, setSetting, useSetting, useStore } from '@/data/store'
import type { Model, SignalLevel, Step, Task } from '@/data/types'

export const cx = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(' ')

// ───────────── motion helpers ─────────────
/** What was shown stays a moment after it goes, marked as leaving, so it can leave the way it came. */
export function useLeaving<T>(value: T | null, ms = 200): [T | null, boolean] {
  const [shown, setShown] = useState<T | null>(value)
  const last = useRef(value)
  useEffect(() => {
    if (value !== null) { last.current = value; setShown(value); return }
    const t = setTimeout(() => setShown(null), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return value !== null ? [value, false] : [shown === null ? null : last.current, shown !== null]
}

/** A ref for a list whose new items come in with motion (CSS: `[data-live] > *` with `@starting-style`). Items
    already there when it appears do not: it turns live a moment after it is put on the screen. */
export function useLiveList(ms = 400) {
  return useCallback((el: HTMLElement | null) => {
    if (!el) return
    el.removeAttribute('data-live')
    setTimeout(() => el.setAttribute('data-live', ''), ms)
  }, [ms])
}

// ───────────── icon and glyph ─────────────
export function Icon({ icon: I, size, label }: { icon: LucideIcon; size?: 'sm' | 'lg' | 'xl'; label?: string }) {
  return (
    <span className={cx('d-icon', size && `d-icon--${size}`)} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true}>
      <I />
    </span>
  )
}

export function Glyph({ name, label, busy, tone, size, style }: { name?: string; label?: string; busy?: boolean; tone?: 'live' | 'caution' | 'danger'; size?: string; style?: CSSProperties }) {
  const color = tone === 'live' ? 'var(--d-color-accent-base)' : tone === 'caution' ? 'var(--d-color-caution-base)' : tone === 'danger' ? 'var(--d-color-danger-base)' : undefined
  return (
    <span
      className="d-glyph"
      data-glyph={name}
      data-busy={busy ? '' : undefined}
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      style={{ ...(color ? { color } : null), ...(size ? ({ '--d-glyph-size': size } as CSSProperties) : null), ...style }}
    />
  )
}

// ───────────── button ─────────────
type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'ghost' | 'danger'
  size?: 'sm' | 'lg'
  iconOnly?: boolean
  pressed?: boolean
}
export function Button({ variant, size, iconOnly, pressed, className, type = 'button', ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      className={cx('d-button', variant && `d-button--${variant}`, size && `d-button--${size}`, iconOnly && 'd-button--icon', className)}
      aria-pressed={pressed}
      {...rest}
    />
  )
}

// ───────────── tooltip ─────────────
export function Tip({ label, children, side = 'top' }: { label: ReactNode; children: React.ReactElement; side?: 'top' | 'right' | 'bottom' | 'left' }) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={children} />
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side={side} sideOffset={6}>
          <BaseTooltip.Popup className="d-tooltip app-pop">{label}</BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  )
}
export const TooltipProvider = BaseTooltip.Provider

// ───────────── menu ─────────────
export type MenuEntry =
  | { kind?: 'item'; label: ReactNode; hint?: ReactNode; lead?: ReactNode; onSelect?: () => void; checked?: boolean; danger?: boolean; disabled?: boolean }
  | { kind: 'label'; label: string }
  | { kind: 'sep' }
/** The entries of a menu, as one menu or another draws them (both share Base UI's menu parts). */
function MenuItems({ items }: { items: MenuEntry[] }) {
  return items.map((it, i) =>
    it.kind === 'sep' ? (
      <hr key={i} className="d-menu__sep" />
    ) : it.kind === 'label' ? (
      <p key={i} className="d-menu__label">{it.label}</p>
    ) : (
      <BaseMenu.Item key={i} className="d-menu__item" onClick={it.onSelect} disabled={it.disabled} data-tone={it.danger ? 'danger' : undefined} aria-checked={it.checked}>
        {it.lead ?? <span style={{ width: 0 }} />}
        <span>{it.label}</span>
        {it.hint && <span className="d-menu__hint">{it.hint}</span>}
      </BaseMenu.Item>
    ),
  )
}

export function Menu({ trigger, items, align = 'start', side = 'bottom' }: { trigger: React.ReactElement; items: MenuEntry[]; align?: 'start' | 'center' | 'end'; side?: 'top' | 'bottom' }) {
  return (
    <BaseMenu.Root>
      <BaseMenu.Trigger render={trigger} />
      <BaseMenu.Portal>
        <BaseMenu.Positioner align={align} side={side} sideOffset={6} className="app-layer">
          <BaseMenu.Popup className="d-menu app-pop">
            <MenuItems items={items} />
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  )
}

/** What a right click on `children` offers: a menu at the pointer, with the same entries and look as `Menu`.
    With no entries, the right click does what it would do anyway. */
export function ContextMenu({ items, children }: { items: MenuEntry[] | null | undefined; children: React.ReactElement }) {
  if (!items || items.length === 0) return children
  return (
    <BaseContextMenu.Root>
      <BaseContextMenu.Trigger render={children} />
      <BaseContextMenu.Portal>
        <BaseContextMenu.Positioner className="app-layer">
          <BaseContextMenu.Popup className="d-menu app-pop">
            <MenuItems items={items} />
          </BaseContextMenu.Popup>
        </BaseContextMenu.Positioner>
      </BaseContextMenu.Portal>
    </BaseContextMenu.Root>
  )
}

/** The text selected inside an element, if any: what "Copy" copies before the whole of it. */
export function selectedWithin(el: Element | null): string {
  const sel = window.getSelection()
  if (!el || !sel || sel.isCollapsed || sel.rangeCount === 0) return ''
  return el.contains(sel.getRangeAt(0).commonAncestorContainer) ? sel.toString() : ''
}

// ───────────── dialog ─────────────
export function Dialog({ open, onOpenChange, title, text, children, actions, wide }: { open: boolean; onOpenChange: (open: boolean) => void; title: string; text?: ReactNode; children?: ReactNode; actions?: ReactNode; wide?: boolean }) {
  return (
    <BaseDialog.Root open={open} onOpenChange={onOpenChange}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="app-scrim" />
        <BaseDialog.Popup className={cx('d-dialog app-dialog', wide && 'app-dialog--wide')}>
          <BaseDialog.Title className="d-dialog__title">{title}</BaseDialog.Title>
          {text && <BaseDialog.Description className="d-dialog__text">{text}</BaseDialog.Description>}
          {children}
          {actions && <div className="d-dialog__actions">{actions}</div>}
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  )
}

// ───────────── model chip ─────────────
export const MODEL_STATE_LABEL: Record<Model['state'], string> = {
  unloaded: 'Unloaded', queued: 'Waiting for memory', loading: 'Loading', loaded: 'Loaded', generating: 'Generating', lent: 'Lent', error: 'Failed to load',
}
export function ModelChip({ model, ...rest }: { model: Model } & ButtonHTMLAttributes<HTMLButtonElement>) {
  const meta = model.state === 'loading' ? `${model.loadProgress ?? 0}%` : model.state === 'lent' ? `lent to ${model.lentTo ?? 'an agent'}` : (model.installed ?? 'not downloaded')
  return (
    <button type="button" className="d-modelchip" data-state={model.state} aria-label={`${model.name}, ${MODEL_STATE_LABEL[model.state]}`} {...rest}>
      <span className="d-glyph" aria-hidden="true" />
      <span className="d-modelchip__name">{model.name}</span>
      <span className="d-modelchip__meta">{meta}</span>
    </button>
  )
}

// ───────────── tasks, steps ─────────────
const TASK_LABEL: Record<Task['state'], string> = { pending: 'Pending', running: 'In progress', done: 'Done', failed: 'Failed', 'needs-input': 'Needs you', skipped: 'Skipped', delegated: 'With an agent' }
export function Tasks({ tasks }: { tasks: Task[] }) {
  return (
    <ul className="d-tasks">
      {tasks.map((t) => (
        <li key={t.id} className="d-task" data-state={t.state}>
          <div className="d-task__row">
            <span className="d-glyph" role="img" aria-label={TASK_LABEL[t.state]} />
            <span className="d-task__label">{t.label}</span>
            {t.meta && <span className="d-task__meta">{t.meta}</span>}
          </div>
          {t.children && <Tasks tasks={t.children} />}
        </li>
      ))}
    </ul>
  )
}

const UNTRUSTED: Record<NonNullable<Step['untrusted']>, string> = { web: 'From the web', email: 'From an email', file: 'From a file', session: 'From another session' }
export function StepRow({ step, onFull }: { step: Step; onFull?: () => void }) {
  return (
    <details className="d-step" data-state={step.state}>
      <summary>
        <span className="d-glyph" role="img" aria-label={step.state === 'running' ? 'In progress' : step.state === 'done' ? 'Done' : step.state === 'failed' ? 'Failed' : 'Needs you'} />
        <span className="d-step__what">
          {step.what} {step.code && <code>{step.code}</code>}
        </span>
        <span className="d-step__meta">{step.meta}</span>
      </summary>
      <div className="d-step__body">
        {step.untrusted && <span className="d-untrusted">{UNTRUSTED[step.untrusted]} · treated as data, never as instructions</span>}
        {step.request && (
          <>
            <p className="d-step__label">Request</p>
            <pre className="d-code"><code>{step.request}</code></pre>
          </>
        )}
        {step.output ? (
          <>
            <p className="d-step__label">Output{step.fullLines ? ' · compressed' : ''}</p>
            <pre className="d-code"><code>{step.output}</code></pre>
            {step.fullLines && (
              <Button size="sm" onClick={onFull}>
                Show full output ({step.fullLines.toLocaleString()} lines)
              </Button>
            )}
          </>
        ) : (
          <p className="d-text-secondary">Running. The output appears here.</p>
        )}
      </div>
    </details>
  )
}

// ───────────── signal, not built ─────────────
export function Signal({ level, children, actions }: { level: SignalLevel; children: ReactNode; actions?: ReactNode }) {
  return (
    <div className="d-signal" data-level={level} role={level === 'error' ? 'alert' : 'note'}>
      <span className="d-signal__label">{level.toUpperCase()}</span>
      <div className="d-signal__body">
        {children}
        {actions && <div className="d-signal__actions">{actions}</div>}
      </div>
    </div>
  )
}
export function NotBuilt({ children, block, label = 'Not built yet' }: { children?: ReactNode; block?: boolean; label?: string }) {
  if (block)
    return (
      <div className="d-notbuilt d-notbuilt--block">
        <strong>
          <Glyph name="task-pending" />
          {label}
        </strong>
        <span>{children}</span>
      </div>
    )
  return (
    <span className="d-notbuilt">
      <Glyph name="task-pending" />
      {children ?? label}
    </span>
  )
}

// ───────────── form controls ─────────────
export function Segmented<T extends string>({ value, onChange, options, label, small, fill }: { value: T; onChange: (v: T) => void; options: { value: T; label: string }[]; label: string; small?: boolean; fill?: boolean }) {
  const name = useId()
  return (
    <div className={cx('d-segmented', small && 'd-segmented--sm', fill && 'd-segmented--fill')} role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <label key={o.value}>
          <input type="radio" name={name} checked={value === o.value} onChange={() => onChange(o.value)} />
          <span>{o.label}</span>
        </label>
      ))}
    </div>
  )
}
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return <input className="d-switch" type="checkbox" role="switch" aria-label={label} checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
}
export function Select<T extends string>({ value, onChange, options, label }: { value: T; onChange: (v: T) => void; options: { value: T; label: string }[]; label: string }) {
  return (
    <span className="d-select">
      <select className="d-input" aria-label={label} value={value} onChange={(e) => onChange(e.target.value as T)}>
        {options.map((o) => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
    </span>
  )
}
export function Slider({ value, onChange, min, max, step, label, format, mark }: { value: number; onChange: (v: number) => void; min: number; max: number; step: number; label: string; format: (v: number) => string; mark?: number }) {
  const pct = (v: number) => `${((v - min) / (max - min)) * 100}%`
  return (
    <div className="d-slider" style={{ flex: 1 }}>
      <input type="range" aria-label={label} min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))}
        style={{ '--d-slider-fill': pct(value), ...(mark !== undefined ? { '--d-slider-mark': pct(mark) } : null) } as CSSProperties} />
      <output className="d-slider__value">{format(value)}</output>
    </div>
  )
}

// ───────────── memory budget ─────────────
export function Budget({ compact }: { compact?: boolean }) {
  const models = useStore((s) => s.models)
  const agents = useStore((s) => s.agents)
  const b = memoryBudget(models, agents)
  const pct = (n: number) => `${(n / b.total) * 100}%`
  const used = b.total - b.free
  return (
    <div className="d-budget">
      <div className="d-budget__bar" role="img" aria-label={`Memory: ${used.toFixed(1)} of ${b.total.toFixed(1)} GB used`}>
        <span data-part="model" style={{ '--d-part': pct(b.model) } as CSSProperties} />
        <span data-part="agent" style={{ '--d-part': pct(b.agent) } as CSSProperties} />
        <span data-part="aux" style={{ '--d-part': pct(b.aux) } as CSSProperties} />
        <span data-part="reserve" style={{ '--d-part': pct(b.reserve) } as CSSProperties} />
      </div>
      {!compact && (
        <dl className="d-budget__legend">
          {b.live.length === 0 && <div><dt>No model loaded</dt><dd>0.0 GB</dd></div>}
          {b.live.map((m) => (
            <div key={m.id} data-part="model"><dt>{m.name}{m.state === 'loading' ? ', loading' : m.state === 'lent' ? ', lent' : ''}</dt><dd>{(m.quants.find((q) => q.id === m.installed)?.memoryGb ?? 0).toFixed(1)} GB</dd></div>
          ))}
          {agents.filter((a) => a.state !== 'done').map((a) => (
            <div key={a.id} data-part="agent"><dt>Agent context</dt><dd>{a.memoryGb.toFixed(1)} GB</dd></div>
          ))}
          <div data-part="aux"><dt>Embedding model</dt><dd>{b.aux.toFixed(1)} GB</dd></div>
          <div data-part="reserve"><dt>Kept free for the system</dt><dd>{b.reserve.toFixed(1)} GB</dd></div>
          <div><dt>Free</dt><dd>{b.free.toFixed(1)} GB</dd></div>
        </dl>
      )}
    </div>
  )
}

// ───────────── page scaffolding ─────────────
/** Whether a screen's right-hand panel is open. Remembered per screen. */
export function useInspector(screen: string) {
  const open = useSetting(`ui.inspector.${screen}`, true)
  return [open, () => setSetting(`ui.inspector.${screen}`, !open)] as const
}
export function InspectorToggle({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <Tip label={open ? 'Hide the side panel' : 'Show the side panel'} side="bottom">
      <Button variant="ghost" iconOnly aria-label={open ? 'Hide the side panel' : 'Show the side panel'} aria-expanded={open} onClick={onToggle}>
        <Icon icon={open ? PanelRightClose : PanelRight} />
      </Button>
    </Tip>
  )
}

/** More about something, one press away: an ⓘ that opens a short read. Keeps screens to a line each. */
export function About({ children, label = 'About this', side = 'bottom' }: { children: ReactNode; label?: string; side?: 'top' | 'bottom' | 'right' | 'left' }) {
  return (
    <BasePopover.Root>
      <BasePopover.Trigger className="d-button d-button--ghost d-button--sm d-button--icon app-about__button" aria-label={label}>
        <Icon icon={Info} size="sm" />
      </BasePopover.Trigger>
      <BasePopover.Portal>
        <BasePopover.Positioner side={side} align="start" sideOffset={6} className="app-layer">
          <BasePopover.Popup className="d-menu app-pop app-about">{children}</BasePopover.Popup>
        </BasePopover.Positioner>
      </BasePopover.Portal>
    </BasePopover.Root>
  )
}

/** While a list is read: the shape of what is coming, shimmering calmly. Says "Loading" to a screen reader. */
export function Skeleton({ rows = 4, lead = false }: { rows?: number; lead?: boolean }) {
  return (
    <div className="app-skeleton" role="status" aria-label="Loading">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="app-skeleton__row" style={{ '--_w': `${[78, 62, 70, 54, 66][i % 5]}%` } as CSSProperties}>
          {lead && <span className="app-skeleton__lead" />}
          <span className="app-skeleton__lines"><span /><span /></span>
        </div>
      ))}
    </div>
  )
}

/** Nothing here yet: a picture of what would be here, what it is in a few words, and the way to make one. */
export function Empty({ icon, title, text, action, compact }: { icon: LucideIcon; title: string; text?: ReactNode; action?: ReactNode; compact?: boolean }) {
  return (
    <div className={cx('app-blank', compact && 'app-blank--compact')}>
      <span className="app-blank__art" aria-hidden="true"><Icon icon={icon} size="xl" /></span>
      <p className="app-blank__title">{title}</p>
      {text && <p className="app-blank__text">{text}</p>}
      {action && <div className="app-blank__action">{action}</div>}
    </div>
  )
}

/** A text's first sentence, with the rest one press away behind an ⓘ: for the notes cards carry. */
export function Brief({ text, className = 'd-text-secondary' }: { text: string | null | undefined; className?: string }) {
  if (!text) return null
  const cut = text.search(/[.;:](\s)/)
  if (cut < 20 || cut > text.length - 12) return <p className={className}>{text}</p>
  const first = text.slice(0, cut + 1).replace(/[;:]$/, '.')
  return <p className={cx(className, 'app-brief')}>{first}<About label="More about this">{text}</About></p>
}

export function Page({ title, lead, about, actions, children, narrow }: { title: string; lead?: ReactNode; about?: ReactNode; actions?: ReactNode; children: ReactNode; narrow?: boolean }) {
  return (
    <div className="app-page">
      <div className={cx('app-page__inner', narrow && 'app-page__inner--narrow')}>
        <header className="app-page__head">
          <div>
            <h1 className="d-type-title-2 app-page__title">{title}{about && <About label={`About ${title}`}>{about}</About>}</h1>
            {lead && <p className="app-page__lead">{lead}</p>}
          </div>
          {actions && <div className="app-page__actions">{actions}</div>}
        </header>
        {children}
      </div>
    </div>
  )
}
export function Section({ title, hint, about, action, children }: { title: string; hint?: ReactNode; about?: ReactNode; action?: ReactNode; children: ReactNode }) {
  // A hint longer than a glance goes behind the ⓘ: a section's head is one line.
  if (typeof hint === 'string' && hint.length > 64) { about = about ?? hint; hint = undefined }
  return (
    <section className="app-section">
      <header className="app-section__head">
        <h2 className="d-type-micro d-text-tertiary app-section__title">{title}{about && <About label={`About ${title.toLowerCase()}`}>{about}</About>}</h2>
        {hint && <span className="d-type-caption d-text-tertiary">{hint}</span>}
        {action && <span className="app-section__action">{action}</span>}
      </header>
      {children}
    </section>
  )
}
/** Name and value, a row each; a row may say more behind an ⓘ beside its name. */
export function Facts({ rows }: { rows: ([string, ReactNode] | [string, ReactNode, ReactNode])[] }) {
  return (
    <dl className="app-facts">
      {rows.map(([k, v, more]) => (
        <div key={k}><dt>{k}{more ? <About label={`About ${k.toLowerCase()}`} side="right">{more}</About> : null}</dt><dd>{v}</dd></div>
      ))}
    </dl>
  )
}
export function SettingRow({ label, desc, details, changed, onReset, children, wide }: { label: string; desc?: ReactNode; details?: ReactNode; changed?: boolean; onReset?: () => void; children?: ReactNode; wide?: boolean }) {
  return (
    <ContextMenu items={onReset ? [{ label: 'Reset to the default', disabled: !changed, onSelect: onReset }] : null}>
    <div className="d-setting">
      <div className="d-setting__text">
        <span className="d-setting__label">
          {label}
          {changed && (
            <button type="button" className="d-setting__changed app-reset" onClick={onReset} title="Reset to default">Changed · Reset</button>
          )}
        </span>
        {desc && <span className="d-setting__desc">{desc}</span>}
      </div>
      {children && <div className="d-setting__control" style={wide ? { width: '15rem' } : undefined}>{children}</div>}
      {details && (
        <details className="d-disclosure d-setting__more">
          <summary>Details</summary>
          <div className="d-disclosure__body">{details}</div>
        </details>
      )}
    </div>
    </ContextMenu>
  )
}
