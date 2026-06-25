import { open, save } from '@tauri-apps/plugin-dialog'
import { ModelSelector } from './ModelSelector'
import { useConversations } from '../../stores/conversations'
import { useArtifacts } from '../../stores/artifacts'
import { exportChat } from '../../lib/tauri'
import { Button } from '@/components/ui/button'

type AgentMode = 'off' | 'cautious' | 'balanced' | 'autonomous'

const AGENT_MODES: { value: AgentMode; label: string; icon: string }[] = [
  { value: 'off',        label: 'Off',        icon: '○'  },
  { value: 'cautious',   label: 'Cautious',   icon: '🔐' },
  { value: 'balanced',   label: 'Balanced',   icon: '⚡' },
  { value: 'autonomous', label: 'Autonomous', icon: '🤖' },
]

export function ChatHeader() {
  const { conversations, activeId, setAgentMode, setWorkingDirectory } = useConversations()
  const conversation = conversations.find(c => c.id === activeId) ?? null
  const artifactOpen = useArtifacts(s => s.activeArtifact !== null)

  const mode: AgentMode = conversation?.agent_mode ?? 'off'
  const workingDir: string | null = conversation?.working_directory ?? null

  const currentMode = AGENT_MODES.find(m => m.value === mode) ?? AGENT_MODES[0]

  const folderName = workingDir
    ? (workingDir.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? workingDir)
    : 'Set folder…'

  async function handlePickFolder() {
    if (!activeId) return
    const result = await open({ directory: true, multiple: false })
    if (typeof result === 'string') {
      await setWorkingDirectory(activeId, result)
    }
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
        {/* Agent mode dropdown — pure CSS hover */}
        <div className="relative group">
          <Button
            variant="ghost"
            size="sm"
            aria-haspopup="listbox"
            aria-label="Agent mode"
            className="gap-1.5 bg-[#1b1b1b]"
          >
            <span>{currentMode.icon}</span>
            <span>{currentMode.label}</span>
            <svg className="w-3 h-3 opacity-60" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5">
              <path d="M3 5l3 3 3-3" />
            </svg>
          </Button>

          <div className="absolute right-0 top-full mt-1 z-50 min-w-[140px] rounded-lg border border-border bg-popover shadow-xl opacity-0 pointer-events-none group-hover:opacity-100 group-hover:pointer-events-auto group-focus-within:opacity-100 group-focus-within:pointer-events-auto transition-opacity">
            {AGENT_MODES.map(m => (
              <button
                key={m.value}
                onClick={() => handleModeSelect(m.value)}
                className={`flex items-center gap-2 w-full px-3 py-2 text-sm text-left hover:bg-accent transition-colors first:rounded-t-lg last:rounded-b-lg ${mode === m.value ? 'text-foreground font-medium' : 'text-muted-foreground'}`}
              >
                <span>{m.icon}</span>
                <span>{m.label}</span>
                {mode === m.value && (
                  <svg className="ml-auto w-3 h-3" viewBox="0 0 12 12" fill="currentColor">
                    <path d="M2 6l3 3 5-5" stroke="currentColor" strokeWidth="1.5" fill="none" />
                  </svg>
                )}
              </button>
            ))}
          </div>
        </div>

        {mode !== 'off' && (
          <Button
            onClick={handlePickFolder}
            title={workingDir ?? 'Pick working directory'}
            variant="ghost"
            size="sm"
            className="gap-1.5 max-w-[180px] text-muted-foreground bg-[#1b1b1b]"
          >
            <svg className="w-3.5 h-3.5 shrink-0" viewBox="0 0 16 16" fill="currentColor">
              <path d="M1.5 3A1.5 1.5 0 000 4.5v8A1.5 1.5 0 001.5 14h13a1.5 1.5 0 001.5-1.5V6a1.5 1.5 0 00-1.5-1.5H7.621a1.5 1.5 0 01-1.06-.44L5.5 3H1.5z" />
            </svg>
            <span className="truncate">{folderName}</span>
          </Button>
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
