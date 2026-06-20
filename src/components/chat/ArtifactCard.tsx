import { FileCode } from 'lucide-react'
import type { Artifact } from '../../types'
import { useArtifacts } from '../../stores/artifacts'

interface Props {
  artifact: Artifact
  version?: number
}

export function ArtifactCard({ artifact, version }: Props) {
  const setActive = useArtifacts(s => s.setActive)
  const activeId = useArtifacts(s => s.activeArtifact?.id)
  const isActive = activeId === artifact.id

  return (
    <button
      onClick={() => setActive(isActive ? null : artifact)}
      className={`flex items-center gap-2 mt-2 px-3 py-2 rounded-lg border text-left transition-colors w-full max-w-xs
        ${isActive
          ? 'bg-primary/10 border-primary/40 text-foreground'
          : 'bg-secondary border-border text-foreground hover:bg-secondary/80'
        }`}
    >
      <FileCode size={14} className="shrink-0 text-muted-foreground" />
      <div className="flex-1 min-w-0">
        <p className="text-xs font-medium truncate">{artifact.title}</p>
        <p className="text-[10px] text-muted-foreground">{artifact.type}</p>
      </div>
      {version !== undefined && (
        <span className="text-[10px] text-muted-foreground/60 shrink-0">v{version}</span>
      )}
    </button>
  )
}
