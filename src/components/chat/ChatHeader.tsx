import { save } from '@tauri-apps/plugin-dialog'
import { ModelSelector } from './ModelSelector'
import { CavemanSelector } from './CavemanSelector'
import { AgentModeSelector, type AgentMode } from './AgentModeSelector'
import { WorkingFolderButton } from './WorkingFolderButton'
import { useConversations } from '../../stores/conversations'
import { useArtifacts } from '../../stores/artifacts'
import type { CavemanLevel } from '../../types'
import { exportChat } from '../../lib/tauri'
import { Button } from '@/components/ui/button'

export function ChatHeader() {
  const { conversations, activeId, setAgentMode, setCavemanLevel, setWorkingDirectory } = useConversations()
  const conversation = conversations.find(c => c.id === activeId) ?? null
  const artifactOpen = useArtifacts(s => s.activeArtifact !== null)

  const mode: AgentMode = conversation?.agent_mode ?? 'off'
  const workingDir: string | null = conversation?.working_directory ?? null
  const cavemanLevel: CavemanLevel = conversation?.caveman_level ?? 'off'

  async function handleCavemanSelect(selected: CavemanLevel) {
    if (!activeId) return
    await setCavemanLevel(activeId, selected)
  }

  async function handleModeSelect(selected: AgentMode) {
    if (!activeId) return
    await setAgentMode(activeId, selected)
    if (selected === 'off' && workingDir !== null) {
      await setWorkingDirectory(activeId, null)
    }
  }

  return (
    <div data-tauri-drag-region className="flex items-center h-12 px-4 border-b border-border shrink-0 gap-2">
      <ModelSelector />

      <div className={`ml-auto flex items-center gap-2 ${artifactOpen ? '' : 'mr-24'}`}>
        <CavemanSelector value={cavemanLevel} onChange={handleCavemanSelect} />
        <AgentModeSelector value={mode} onChange={handleModeSelect} />

        {mode !== 'off' && (
          <WorkingFolderButton
            value={workingDir}
            onChange={path => { if (activeId) setWorkingDirectory(activeId, path) }}
          />
        )}

        <Button
          onClick={async () => {
            if (!activeId) return
            const filePath = await save({
              filters: [{ name: 'JSON', extensions: ['json'] }],
              defaultPath: 'chat-export.json',
            })
            if (filePath) {
              await exportChat.exportConversation(activeId, filePath)
            }
          }}
          title="Export conversation as JSON"
          variant="ghost"
          size="icon-sm"
          className="text-muted-foreground bg-[#1b1b1b]"
        >
          <svg className="w-3.5 h-3.5" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5">
            <path d="M8 2v9M4 7l4 4 4-4M2 12v1.5A1.5 1.5 0 003.5 15h9a1.5 1.5 0 001.5-1.5V12" />
          </svg>
        </Button>
      </div>
    </div>
  )
}
