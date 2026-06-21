import { useState } from 'react'
import JSON5 from 'json5'
import { ChevronRight, ChevronDown } from 'lucide-react'
import { cn } from '../../lib/utils'

type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue }

function JsonNode({ value, keyName, depth = 0 }: { value: JsonValue; keyName?: string; depth?: number }) {
  const [open, setOpen] = useState(depth < 2)

  const isObj = value !== null && typeof value === 'object'
  const isArr = Array.isArray(value)

  const entries = isObj ? (isArr ? (value as JsonValue[]).map((v, i) => [String(i), v] as [string, JsonValue]) : Object.entries(value as Record<string, JsonValue>)) : []
  const count = entries.length

  const typeColor = (v: JsonValue) => {
    if (v === null) return 'text-red-400'
    if (typeof v === 'boolean') return 'text-purple-400'
    if (typeof v === 'number') return 'text-blue-400'
    return 'text-green-400'
  }

  const primitive = !isObj
  const label = keyName !== undefined ? (
    <span className="text-yellow-300/80 font-mono text-xs">{keyName}</span>
  ) : null

  if (primitive) {
    return (
      <div className="flex items-center gap-1 py-0.5 pl-1">
        {label && <>{label}<span className="text-muted-foreground text-xs">:</span></>}
        <span className={cn('font-mono text-xs', typeColor(value))}>
          {value === null ? 'null' : typeof value === 'string' ? `"${value}"` : String(value)}
        </span>
      </div>
    )
  }

  const bracket = isArr ? ['[', ']'] : ['{', '}']

  return (
    <div>
      <div
        className="flex items-center gap-1 py-0.5 cursor-pointer hover:bg-white/5 rounded pl-1 group"
        onClick={() => setOpen(o => !o)}
      >
        <span className="text-muted-foreground w-3 h-3 flex items-center justify-center shrink-0">
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
        </span>
        {label && <>{label}<span className="text-muted-foreground text-xs">:</span></>}
        <span className="text-muted-foreground font-mono text-xs">{bracket[0]}</span>
        {!open && (
          <>
            <span className="text-muted-foreground font-mono text-xs opacity-60">
              {count} {isArr ? 'item' : 'key'}{count !== 1 ? 's' : ''}
            </span>
            <span className="text-muted-foreground font-mono text-xs">{bracket[1]}</span>
          </>
        )}
      </div>
      {open && (
        <div className="border-l border-border/40 ml-3 pl-2">
          {entries.map(([k, v]) => (
            <JsonNode key={k} value={v} keyName={k} depth={depth + 1} />
          ))}
        </div>
      )}
      {open && (
        <div className="pl-4">
          <span className="text-muted-foreground font-mono text-xs">{bracket[1]}</span>
        </div>
      )}
    </div>
  )
}

export function JsonTreeViewer({ content }: { content: string }) {
  let parsed: JsonValue
  try {
    parsed = JSON5.parse(content)
  } catch {
    return (
      <div className="p-4 text-red-400 text-sm font-mono">Invalid JSON</div>
    )
  }

  return (
    <div className="p-4 overflow-auto h-full">
      <JsonNode value={parsed} depth={0} />
    </div>
  )
}
