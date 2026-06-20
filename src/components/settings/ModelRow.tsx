import { useState } from 'react'
import { Pencil, Check, X, Eye, Wrench, Brain } from 'lucide-react'
import type { ModelOverride } from '../../types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useProviders } from '../../stores/providers'

interface Props {
  providerId: string
  modelId: string
  override?: ModelOverride
  onUpdate: (override: ModelOverride) => void
}

export function ModelRow({ providerId, modelId, override, onUpdate }: Props) {
  const [editing, setEditing] = useState(false)
  const [editValue, setEditValue] = useState(override?.custom_name ?? '')
  const { modelCapabilities, providers } = useProviders()

  const enabled = override?.enabled ?? true
  const customName = override?.custom_name

  const provider = providers.find(p => p.id === providerId)
  const caps = modelCapabilities[providerId]?.[modelId]

  const visionSupported = caps?.vision ?? (provider?.type === 'anthropic' ? true : undefined)
  const toolsSupported = caps?.tools ?? (provider?.type === 'anthropic' ? true : undefined)
  const reasoningSupported = caps?.reasoning ?? (provider?.type === 'anthropic' ? true : undefined)

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
          <div className="flex items-center gap-1 shrink-0 ml-1">
            {visionSupported && <span title="Supports vision"><Eye size={11} className="text-muted-foreground/50" /></span>}
            {toolsSupported && <span title="Supports tool calling"><Wrench size={11} className="text-muted-foreground/50" /></span>}
            {reasoningSupported && <span title="Supports reasoning/thinking"><Brain size={11} className="text-muted-foreground/50" /></span>}
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
