import { useState } from 'react'
import { Pencil, Check, X, Eye, Wrench, Brain } from 'lucide-react'
import type { ModelOverride, ModelCaps, CapsSource, CapName } from '../../types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useProviders } from '../../stores/providers'

/// Say where a capability flag came from, so "no vision icon" can be read as either
/// "the host says no" or "nobody knows".
export const CAPS_SOURCE_LABEL: Record<CapsSource, string> = {
  provider: 'reported by the provider',
  llamaCpp: 'detected from the loaded model by llama.cpp',
  registry: 'from the models.dev registry',
  huggingFace: "read from the repo's config on Hugging Face",
  unknown: 'unknown: not reported by the provider or listed on models.dev',
}

const CAPS_UI: { name: CapName; icon: typeof Eye; label: string }[] = [
  { name: 'vision', icon: Eye, label: 'Vision' },
  { name: 'tools', icon: Wrench, label: 'Tool calling' },
  { name: 'reasoning', icon: Brain, label: 'Reasoning' },
]

/// Click cycles auto → yes → no → auto. `null` hands the flag back to detection.
function nextOverride(current: boolean | null | undefined): boolean | null {
  if (current === null || current === undefined) return true
  return current ? false : null
}

function capTooltip(cap: string, on: boolean, overridden: boolean, source: CapsSource): string {
  const verdict = on ? `Supports ${cap}` : `No ${cap}`
  const why = overridden
    ? 'you set this'
    : source === 'unknown'
      ? 'unknown: nothing reported this model'
      : CAPS_SOURCE_LABEL[source]
  return `${verdict}: ${why}. Click to change.`
}

interface Props {
  providerId: string
  modelId: string
  override?: ModelOverride
  onUpdate: (override: ModelOverride) => void
}

export function ModelRow({ providerId, modelId, override, onUpdate }: Props) {
  const [editing, setEditing] = useState(false)
  const [editValue, setEditValue] = useState(override?.custom_name ?? '')
  const { modelCapabilities, setModelCapsOverride } = useProviders()

  const enabled = override?.enabled ?? true
  const customName = override?.custom_name

  const caps: ModelCaps | undefined = modelCapabilities[providerId]?.[modelId]

  const cycleCap = (name: CapName) => {
    const current = override?.[`caps_${name}` as const]
    setModelCapsOverride(providerId, modelId, { [name]: nextOverride(current) })
  }

  const handleToggle = () => {
    onUpdate({ provider_id: providerId, model_id: modelId, custom_name: customName, enabled: !enabled })
  }

  const handleEditSave = () => {
    const trimmed = editValue.trim()
    onUpdate({ provider_id: providerId, model_id: modelId, custom_name: trimmed || undefined, enabled: override?.enabled ?? true })
    setEditing(false)
  }

  const handleEditCancel = () => {
    setEditValue(override?.custom_name ?? '')
    setEditing(false)
  }

  return (
    <div className="flex items-center gap-2 py-1.5 group">
      <Switch checked={enabled} onCheckedChange={handleToggle} className="shrink-0 scale-75" />

      {editing ? (
        <div className="flex items-center gap-1 flex-1 min-w-0">
          <Input
            autoFocus
            type="text"
            value={editValue}
            onChange={e => setEditValue(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') handleEditSave(); if (e.key === 'Escape') handleEditCancel() }}
            placeholder={modelId}
            className="flex-1 min-w-0 h-6 text-xs"
          />
          <Button onClick={handleEditSave} variant="ghost" size="icon-xs" className="text-primary"><Check size={13} /></Button>
          <Button onClick={handleEditCancel} variant="ghost" size="icon-xs" className="text-muted-foreground"><X size={13} /></Button>
        </div>
      ) : (
        <div className="flex items-center gap-1.5 flex-1 min-w-0">
          {customName ? (
            <span className="flex items-baseline gap-1.5 min-w-0 flex-wrap">
              <span className="text-xs text-foreground shrink-0">{customName}</span>
              <span className="text-xs text-muted-foreground/60 shrink-0">{modelId}</span>
            </span>
          ) : (
            <span className="text-xs text-foreground truncate">{modelId}</span>
          )}
          {/* All three always render: a capability the model lacks has to be visible to be
              correctable, and the user is the last word on any of them. */}
          <div className="flex items-center gap-0.5 shrink-0 ml-1">
            {caps && CAPS_UI.map(({ name, icon: Icon, label }) => {
              const on = caps[name]
              const overridden = caps.overridden[name]
              return (
                <button
                  key={name}
                  onClick={() => cycleCap(name)}
                  title={capTooltip(label.toLowerCase(), on, overridden, caps.source)}
                  aria-label={capTooltip(label.toLowerCase(), on, overridden, caps.source)}
                  aria-pressed={on}
                  className="p-0.5 rounded hover:bg-muted/60 transition-colors"
                >
                  <Icon
                    size={11}
                    // Off is off, same gray however it was decided. Only a user-set "yes"
                    // gets the accent; the tooltip carries the rest.
                    className={
                      on
                        ? (overridden ? 'text-primary' : 'text-muted-foreground/60')
                        : 'text-muted-foreground/15'
                    }
                  />
                </button>
              )
            })}
          </div>
          <Button
            onClick={() => { setEditValue(customName ?? ''); setEditing(true) }}
            variant="ghost"
            size="icon-xs"
            className="opacity-0 group-hover:opacity-100 text-muted-foreground shrink-0 transition-opacity"
          >
            <Pencil size={11} />
          </Button>
        </div>
      )}
    </div>
  )
}
