import { useRef, useState, useEffect, useMemo } from 'react'
import { ChevronDown, ChevronRight, Layers, Globe, Mail, Plug, LucideIcon } from 'lucide-react'
import Fuse from 'fuse.js'
import { invoke } from '@tauri-apps/api/core'
import { useMcpTools } from '../../stores/mcpTools'
import {
  useSkills,
  skillIdOfServer,
  type SkillToolDef,
  type SkillPromptToolDef,
  type SkillBuiltinToolDef,
} from '../../stores/skills'
import { useBuiltinTools } from '../../stores/builtinTools'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '../ui/tooltip'

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
  const { tools: allBuiltinTools, groupOverrides: builtinGroupOverrides, toggle: toggleBuiltin, toggleGroup: toggleBuiltinGroup } = useBuiltinTools()
  const [connectedServices, setConnectedServices] = useState<string[] | null>(null)
  const [query, setQuery] = useState('')
  const [builtinCollapsed, setBuiltinCollapsed] = useState<Record<string, boolean>>({})
  const [searchTools, setSearchTools] = useState(() => {
    const stored = localStorage.getItem('toolPopup:searchTools')
    return stored === null ? true : stored === 'true'
  })
  const searchRef = useRef<HTMLInputElement>(null)

  useEffect(() => { searchRef.current?.focus() }, [])

  useEffect(() => {
    invoke<{ services: string[] }[]>('list_accounts')
      .then(list => setConnectedServices(Array.from(new Set(list.flatMap(a => a.services)))))
      .catch(() => setConnectedServices([]))
  }, [])

  // Google-backed groups only appear once an account is linked to that service
  const builtinTools = useMemo(() => {
    const svc: Record<string, string> = { Email: 'email', Calendar: 'calendar', Contacts: 'contacts' }
    return allBuiltinTools.filter(t => {
      const need = svc[t.group]
      return !need || !!connectedServices?.includes(need)
    })
  }, [allBuiltinTools, connectedServices])

  // A skill's own servers are listed under that skill, not in MCP Tools — otherwise the same
  // server appears twice and disabling the skill leaves a live-looking row behind.
  const mcpOnlyTools = useMemo(() => tools.filter(t => !skillIdOfServer(t.server_id)), [tools])
  const servers = Array.from(new Set(mcpOnlyTools.map(t => t.server_id)))

  const toolFuse = useMemo(() => new Fuse(mcpOnlyTools, { keys: ['name', 'server_name'], threshold: 0.4 }), [mcpOnlyTools])
  const serverFuse = useMemo(() => {
    const names = servers.map(id => ({ id, name: mcpOnlyTools.find(t => t.server_id === id)?.server_name || id }))
    return new Fuse(names, { keys: ['name'], threshold: 0.4 })
  }, [servers, mcpOnlyTools])

  const builtinGroups = useMemo(
    () => Array.from(new Set(builtinTools.map(t => t.group))),
    [builtinTools]
  )

  /** The live tools of a skill's own MCP servers — empty until its server has spawned. */
  const skillServerTools = (skillId: string) =>
    tools.filter(t => skillIdOfServer(t.server_id) === skillId)

  /**
   * The tools a skill declares directly: its `prompt` bodies and the builtins it surfaces. Its
   * `mcp` entries are servers, so their tools arrive live via `skillServerTools` instead.
   */
  const declaredToolsOf = (skill: { tools: SkillToolDef[] }) =>
    skill.tools.filter(
      (t): t is SkillPromptToolDef | SkillBuiltinToolDef => t.type === 'prompt' || t.type === 'builtin',
    )

  const filteredSkills = useMemo(() => {
    if (!query.trim()) return skills
    const fuse = new Fuse(skills, { keys: ['name', 'description'], threshold: 0.4 })
    return fuse.search(query.trim()).map(r => r.item)
  }, [skills, query])

  if (servers.length === 0 && skills.length === 0 && builtinTools.length === 0) {
    return (
      <div className="absolute bottom-full left-0 mb-2 w-64 bg-secondary border border-border rounded-xl shadow-2xl z-50 p-4">
        <p className="text-xs text-muted-foreground text-center">No tools available</p>
      </div>
    )
  }

  const GROUP_ICONS: Record<string, LucideIcon> = { 'Web Browse': Globe, 'Email': Mail }

  const q = query.trim()
  const matchedServerIds = q ? new Set(serverFuse.search(q).map(r => r.item.id)) : null
  const matchedToolNames = q && searchTools ? new Set(toolFuse.search(q).map(r => r.item.name)) : null

  const filteredServers = servers
    .map(serverId => {
      const serverTools = mcpOnlyTools.filter(t => t.server_id === serverId)
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
    <TooltipProvider delayDuration={400}>
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

        {/* Built In section: always first */}
        {builtinTools.length > 0 && (
          <>
            <div className="px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              Built In
            </div>
            {builtinGroups.map((group, gi) => {
              const groupTools = builtinTools.filter(t => t.group === group)
              const isCollapsible = groupTools.length > 1
              const isCollapsed = (builtinCollapsed[group] ?? true) && !q
              const isGroupOverridden = !!builtinGroupOverrides[group]
              return (
                <div key={group} className={gi < builtinGroups.length - 1 ? 'border-b border-border' : ''}>
                  {isCollapsible ? (
                    <>
                      <div
                        className="flex items-center gap-2 px-3 py-2.5 cursor-pointer hover:bg-accent/50 transition-colors"
                        onClick={() => setBuiltinCollapsed(s => ({ ...s, [group]: !(s[group] ?? true) }))}
                      >
                        {isCollapsed
                          ? <ChevronRight size={12} className="text-muted-foreground shrink-0" />
                          : <ChevronDown size={12} className="text-muted-foreground shrink-0" />
                        }
                        {GROUP_ICONS[group] && (() => { const Icon = GROUP_ICONS[group]; return <Icon size={13} className="text-muted-foreground shrink-0" /> })()}
                        <span className="flex-1 text-sm font-semibold text-foreground truncate">{group}</span>
                        <Toggle enabled={!isGroupOverridden} onToggle={() => toggleBuiltinGroup(group)} />
                      </div>
                      {!isCollapsed && (
                        <div className="ml-4 border-l border-border">
                          {groupTools.map(tool => (
                            <Tooltip key={tool.id}>
                              <TooltipTrigger asChild>
                                <div onClick={() => !isGroupOverridden && toggleBuiltin(tool.id)} className={`flex items-center gap-2 px-3 py-2 hover:bg-accent/50 transition-colors transition-opacity cursor-pointer ${isGroupOverridden ? 'opacity-40 cursor-not-allowed' : ''}`}>
                                  <div className="flex-1 min-w-0">
                                    <p className="text-xs text-foreground/80 truncate">{tool.name}</p>
                                    {tool.description && <p className="text-[10px] text-muted-foreground truncate">{tool.description}</p>}
                                  </div>
                                  <Toggle enabled={tool.enabled} onToggle={() => toggleBuiltin(tool.id)} disabled={isGroupOverridden} />
                                </div>
                              </TooltipTrigger>
                              {tool.description && <TooltipContent side="right" className="max-w-56">{tool.description}</TooltipContent>}
                            </Tooltip>
                          ))}
                        </div>
                      )}
                    </>
                  ) : (
                    groupTools.map(tool => (
                      <Tooltip key={tool.id}>
                        <TooltipTrigger asChild>
                          <div onClick={() => toggleBuiltin(tool.id)} className="flex items-center gap-2 px-3 py-2.5 hover:bg-accent/50 transition-colors cursor-pointer">
                            <div className="flex-1 min-w-0">
                              <p className="text-sm font-semibold text-foreground truncate">{tool.name}</p>
                              {tool.description && (
                                <p className="text-[10px] text-muted-foreground truncate">{tool.description}</p>
                              )}
                            </div>
                            <Toggle enabled={tool.enabled} onToggle={() => toggleBuiltin(tool.id)} />
                          </div>
                        </TooltipTrigger>
                        {tool.description && <TooltipContent side="right" className="max-w-56">{tool.description}</TooltipContent>}
                      </Tooltip>
                    ))
                  )}
                </div>
              )
            })}
          </>
        )}

        {/* MCP Tools section */}
        {filteredServers.length > 0 && (
          <>
            <div className={`px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground ${builtinTools.length > 0 ? 'border-t border-border mt-1' : ''}`}>
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
                        <Tooltip key={tool.name}>
                          <TooltipTrigger asChild>
                            <div onClick={() => !isOverridden && toggleTool(`${tool.server_id}:${tool.name}`)} className={`flex items-center gap-2 px-3 py-2 hover:bg-accent/50 transition-colors transition-opacity cursor-pointer ${isOverridden ? 'opacity-40 cursor-not-allowed' : ''}`}>
                              <div className="flex-1 min-w-0">
                                <p className="text-xs text-foreground/80 truncate">{tool.name}</p>
                                {tool.description && <p className="text-[10px] text-muted-foreground truncate">{tool.description}</p>}
                              </div>
                              <Toggle
                                enabled={tool.enabled}
                                onToggle={() => toggleTool(`${tool.server_id}:${tool.name}`)}
                                disabled={isOverridden}
                              />
                            </div>
                          </TooltipTrigger>
                          {tool.description && <TooltipContent side="right" className="max-w-56">{tool.description}</TooltipContent>}
                        </Tooltip>
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
            <div className={`px-3 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground ${filteredServers.length > 0 || builtinTools.length > 0 ? 'border-t border-border mt-1' : ''}`}>
              Skills
            </div>
            {filteredSkills.map(skill => (
              <div key={skill.id}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <div onClick={() => toggleSkill(skill.id)} className="flex items-center gap-2 px-3 py-2.5 hover:bg-accent/50 transition-colors cursor-pointer">
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-semibold text-foreground truncate">{skill.name}</p>
                        {skill.description && (
                          <p className="text-[10px] text-muted-foreground truncate">{skill.description}</p>
                        )}
                      </div>
                      <Toggle enabled={skill.enabled} onToggle={() => toggleSkill(skill.id)} />
                    </div>
                  </TooltipTrigger>
                  {skill.description && <TooltipContent side="right" className="max-w-56">{skill.description}</TooltipContent>}
                </Tooltip>

                {/* Everything this skill brings: prompt tools from skill.json, then the tools of
                    any MCP server from its mcp.json. Prompt tools are read-only — the skill's
                    toggle is their switch, since the backend offers them only for enabled skills.
                    MCP tools keep individual toggles, like any other MCP tool. */}
                {(declaredToolsOf(skill).length > 0 || skillServerTools(skill.id).length > 0) && (
                  <div className={`ml-4 border-l border-border ${skill.enabled ? '' : 'opacity-40'}`}>
                    {declaredToolsOf(skill).map(tool => (
                      <Tooltip key={tool.name}>
                        <TooltipTrigger asChild>
                          <div className="flex items-center gap-2 px-3 py-1.5">
                            <div className="flex-1 min-w-0">
                              <p className="text-xs text-foreground/80 truncate">{tool.name}</p>
                              {tool.description && <p className="text-[10px] text-muted-foreground truncate">{tool.description}</p>}
                            </div>
                          </div>
                        </TooltipTrigger>
                        {tool.description && <TooltipContent side="right" className="max-w-56">{tool.description}</TooltipContent>}
                      </Tooltip>
                    ))}
                    {skillServerTools(skill.id).map(tool => (
                      <Tooltip key={`${tool.server_id}:${tool.name}`}>
                        <TooltipTrigger asChild>
                          <div
                            onClick={() => skill.enabled && toggleTool(`${tool.server_id}:${tool.name}`)}
                            className={`flex items-center gap-2 px-3 py-1.5 hover:bg-accent/50 transition-colors cursor-pointer ${skill.enabled ? '' : 'cursor-not-allowed'}`}
                          >
                            <Plug size={11} className="text-muted-foreground shrink-0" />
                            <div className="flex-1 min-w-0">
                              <p className="text-xs text-foreground/80 truncate">{tool.name}</p>
                              {tool.description && <p className="text-[10px] text-muted-foreground truncate">{tool.description}</p>}
                            </div>
                            <Toggle
                              enabled={tool.enabled}
                              onToggle={() => toggleTool(`${tool.server_id}:${tool.name}`)}
                              disabled={!skill.enabled}
                            />
                          </div>
                        </TooltipTrigger>
                        {tool.description && <TooltipContent side="right" className="max-w-56">{tool.description}</TooltipContent>}
                      </Tooltip>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </>
        )}
      </div>
    </div>
    </TooltipProvider>
  )
}
