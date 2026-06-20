import { useRef, useState, useEffect, useMemo } from 'react'
import { ChevronDown, ChevronRight, Layers } from 'lucide-react'
import Fuse from 'fuse.js'
import { useMcpTools } from '../../stores/mcpTools'
import { useSkills } from '../../stores/skills'

function Toggle({ enabled, onToggle, disabled }: { enabled: boolean; onToggle: () => void; disabled?: boolean }) {
  return (
    <button
      onClick={e => { e.stopPropagation(); onToggle() }}
      disabled={disabled}
      className={`relative w-8 h-4 rounded-full transition-colors shrink-0 ${
        enabled ? 'bg-primary' : 'bg-secondary'
      } ${disabled ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'}`}
    >
      <span className={`absolute top-0.5 w-3 h-3 rounded-full transition-transform ${
        enabled ? 'bg-white right-0.5' : 'bg-[var(--muted-foreground)] left-0.5'
      }`} />
    </button>
  )
}

export function ToolSelectorPopup() {
  const { tools, collapsed, serverOverrides, toggleTool, toggleServer, toggleCollapse } = useMcpTools()
  const { skills, toggle: toggleSkill } = useSkills()
  const [query, setQuery] = useState('')
  const [searchTools, setSearchTools] = useState(() => {
    const stored = localStorage.getItem('toolPopup:searchTools')
    return stored === null ? true : stored === 'true'
  })
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => { searchRef.current?.focus() }, [])

  const servers = Array.from(new Set(tools.map(t => t.server_id)))

  const toolFuse = useMemo(() => new Fuse(tools, { keys: ['name', 'server_name'], threshold: 0.4 }), [tools])
  const serverFuse = useMemo(() => {
    const names = servers.map(id => ({ id, name: tools.find(t => t.server_id === id)?.server_name || id }))
    return new Fuse(names, { keys: ['name'], threshold: 0.4 })
  }, [servers, tools])

  const filteredSkills = useMemo(() => {
    if (!query.trim()) return skills
    const fuse = new Fuse(skills, { keys: ['name', 'description'], threshold: 0.4 })
    return fuse.search(query.trim()).map(r => r.item)
  }, [skills, query])

  if (servers.length === 0 && skills.length === 0) {
    return (
      <div className="absolute bottom-full left-0 mb-2 w-64 bg-secondary border border-border rounded-xl shadow-2xl z-50 p-4">
        <p className="text-xs text-muted-foreground text-center">No MCP servers or skills available</p>
      </div>
    )
  }

  const q = query.trim()
  const matchedServerIds = q ? new Set(serverFuse.search(q).map(r => r.item.id)) : null
  const matchedToolNames = q && searchTools ? new Set(toolFuse.search(q).map(r => r.item.name)) : null

  const filteredServers = servers
    .map(serverId => {
      const serverTools = tools.filter(t => t.server_id === serverId)
      const serverName = serverTools[0]?.server_name || serverId
      const serverMatches = !q || matchedServerIds!.has(serverId)
      const matchingTools = matchedToolNames
        ? serverTools.filter(t => matchedToolNames.has(t.name))
        : serverTools
      const visible = !q || serverMatches || (searchTools && matchingTools.length > 0)
      return { serverId, serverName, serverTools, matchingTools: serverMatches ? serverTools : matchingTools, visible }
    })
    .filter(s => s.visible)

  return (
    <div className="absolute bottom-full left-0 mb-2 w-72 bg-secondary border border-border rounded-xl shadow-2xl z-50 overflow-hidden flex flex-col max-h-[60vh]">
      <div className="px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground border-b border-border shrink-0">
        Tools
      </div>

      {/* Search bar */}
      <div className="p-2 border-b border-border shrink-0">
        <div className="flex items-center gap-1.5">
          <input
            ref={searchRef}
            type="text"
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={e => e.key === 'Escape' && setQuery('')}
            placeholder="Search servers..."
            className="flex-1 bg-background border border-border rounded-md px-3 py-1.5 text-xs text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50"
          />
          <button
            onClick={() => setSearchTools(s => { const next = !s; localStorage.setItem('toolPopup:searchTools', String(next)); return next })}
            title="Search tools too"
            className={`w-7 h-7 flex items-center justify-center rounded-md border transition-colors shrink-0 ${
              searchTools
                ? 'bg-primary/20 border-[var(--primary)]/60 text-primary'
                : 'bg-background border-border text-muted-foreground hover:text-foreground/80 hover:border-[var(--accent)]'
            }`}
          >
            <Layers size={13} />
          </button>
        </div>
      </div>

      <div className="overflow-y-auto">
        {filteredServers.length === 0 && filteredSkills.length === 0 && q && (
          <p className="px-3 py-3 text-xs text-muted-foreground">No matches for "{query}".</p>
        )}

        {/* MCP Tools section */}
        {filteredServers.length > 0 && (
          <>
            <div className="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              MCP Tools
            </div>
            {filteredServers.map(({ serverId, serverName, matchingTools }, i) => {
              const isCollapsed = (collapsed[serverId] ?? true) && !q
              const isOverridden = !!serverOverrides[serverId]
              const serverEnabled = !isOverridden

              return (
                <div key={serverId} className={i < filteredServers.length - 1 ? 'border-b border-border' : ''}>
                  <div
                    className="flex items-center gap-2 px-3 py-2.5 cursor-pointer hover:bg-accent/50 transition-colors"
                    onClick={() => toggleCollapse(serverId)}
                  >
                    {isCollapsed
                      ? <ChevronRight size={12} className="text-muted-foreground shrink-0" />
                      : <ChevronDown size={12} className="text-muted-foreground shrink-0" />
                    }
                    <span className="flex-1 text-sm font-semibold text-foreground truncate">{serverName}</span>
                    <Toggle enabled={serverEnabled} onToggle={() => toggleServer(serverId)} />
                  </div>

                  {!isCollapsed && (
                    <div className="ml-4 border-l border-border">
                      {matchingTools.map(tool => (
                        <div
                          key={tool.name}
                          className={`flex items-center gap-2 px-3 py-2 transition-opacity ${isOverridden ? 'opacity-40' : ''}`}
                        >
                          <span className="flex-1 text-xs text-foreground/80 truncate">{tool.name}</span>
                          <Toggle
                            enabled={tool.enabled}
                            onToggle={() => toggleTool(`${tool.server_id}:${tool.name}`)}
                            disabled={isOverridden}
                          />
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )
            })}
          </>
        )}

        {/* Skills section */}
        {filteredSkills.length > 0 && (
          <>
            <div className={`px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground ${filteredServers.length > 0 ? 'border-t border-border mt-1' : ''}`}>
              Skills
            </div>
            {filteredSkills.map(skill => (
              <div key={skill.id} className="flex items-center gap-2 px-3 py-2.5 hover:bg-accent/50 transition-colors">
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-semibold text-foreground truncate">{skill.name}</p>
                  {skill.description && (
                    <p className="text-[10px] text-muted-foreground truncate">{skill.description}</p>
                  )}
                </div>
                <Toggle enabled={skill.enabled} onToggle={() => toggleSkill(skill.id)} />
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  )
}
