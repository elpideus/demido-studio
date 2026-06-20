import { Shield } from 'lucide-react'
import { useMessages, type PermissionRequest, type ResolvedPermission } from '../../stores/messages'

export function PendingBubble({ req }: { req: PermissionRequest }) {
  const respondToPermission = useMessages(s => s.respondToPermission)
  return (
    <div className="border border-amber-500/25 bg-amber-950/20 rounded-lg p-3.5 max-w-[90%]">
      <div className="flex items-center gap-2 mb-2.5">
        <Shield size={13} className="text-amber-400 flex-shrink-0" />
        <span className="text-amber-400 text-[11px] font-semibold uppercase tracking-wider">Permission Required</span>
      </div>
      <div className="text-foreground/80 text-[13px] mb-1 font-medium">{req.toolName}</div>
      <div className="bg-black/30 rounded px-2.5 py-1.5 font-mono text-[12px] text-muted-foreground mb-3">{req.description}</div>
      <div className="flex gap-2">
        <button onClick={() => respondToPermission(true)} className="bg-green-700 hover:bg-green-600 text-white text-xs font-medium px-4 py-1.5 rounded transition-colors">Allow</button>
        <button onClick={() => respondToPermission(false)} className="bg-red-700 hover:bg-red-600 text-white text-xs font-medium px-4 py-1.5 rounded transition-colors">Deny</button>
      </div>
    </div>
  )
}

export function ResolvedBadge({ resolved }: { resolved: ResolvedPermission }) {
  return (
    <div className={`flex items-center gap-2 rounded-lg px-3 py-2 text-xs max-w-[90%] border ${resolved.approved ? 'border-green-800/30 bg-green-950/20 text-green-400' : 'border-red-800/30 bg-red-950/20 text-red-400'}`}>
      <span>{resolved.approved ? '✓' : '✕'}</span>
      <span className="font-medium">{resolved.toolName}</span>
      <span className="opacity-60 font-mono">{resolved.description}</span>
    </div>
  )
}

export function PermissionBubbles() {
  const pendingPermission = useMessages(s => s.pendingPermission)
  const resolvedPermissions = useMessages(s => s.resolvedPermissions)
  if (!pendingPermission && resolvedPermissions.length === 0) return null
  return (
    <div className="space-y-2">
      {resolvedPermissions.map((r, i) => <ResolvedBadge key={i} resolved={r} />)}
      {pendingPermission && <PendingBubble req={pendingPermission} />}
    </div>
  )
}
