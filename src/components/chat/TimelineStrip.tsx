import { useState } from 'react'
import { MessageSquare, Wrench, ChevronRight, Zap } from 'lucide-react'
import { MarkdownRenderer } from './MarkdownRenderer'
import { cn } from '../../lib/utils'
import type { StreamBlock, ThinkingBlock, ToolBlock, SkillBlock, ResolvedPermission, PermissionRequest } from '../../stores/messages'
import { ResolvedBadge, PendingBubble } from './PermissionBubble'

// ── Thinking row ──────────────────────────────────────────────────────────────

function ThinkingRow({ block }: { block: ThinkingBlock }) {
  const [open, setOpen] = useState(false)

  const preview = block.content
    ? block.content.split('\n').find(l => l.trim()) ?? 'Thinking'
    : 'Thinking…'

  return (
    <div className="flex flex-col">
      <div
        className={cn('flex items-center gap-2 py-[5px] pr-1', block.done && 'cursor-pointer')}
        onClick={() => block.done && setOpen(o => !o)}
      >
        <div className="w-[18px] h-[18px] flex items-center justify-center rounded bg-secondary border border-border shrink-0">
          <MessageSquare size={9} className="text-muted-foreground/60" />
        </div>

        {!block.done ? (
          <span className="text-[11px] text-muted-foreground/60 flex-1 min-w-0 flex items-center gap-1.5 overflow-hidden">
            <span className="flex gap-0.5 shrink-0">
              <span className="w-1 h-1 rounded-full bg-[var(--muted-foreground)] animate-bounce" style={{ animationDelay: '0ms' }} />
              <span className="w-1 h-1 rounded-full bg-[var(--muted-foreground)] animate-bounce" style={{ animationDelay: '150ms' }} />
              <span className="w-1 h-1 rounded-full bg-[var(--muted-foreground)] animate-bounce" style={{ animationDelay: '300ms' }} />
            </span>
            <span className="truncate text-muted-foreground">Thinking…</span>
          </span>
        ) : (
          <span className="text-[11px] text-muted-foreground/60 flex-1 truncate">{open ? 'Thinking' : preview}</span>
        )}

        {block.done && (
          <ChevronRight
            size={10}
            className={cn('text-[var(--accent)] shrink-0 transition-transform', open && 'rotate-90')}
          />
        )}
      </div>

      {open && block.done && (
        <div
          className="ml-[26px] mb-[6px] px-3 py-2.5 bg-background border border-[var(--secondary)] rounded-lg text-[11px] text-muted-foreground/70 leading-relaxed"
          style={{
            '--tw-prose-body': '#6b6b88',
            '--tw-prose-bold': '#6b6b88',
            '--tw-prose-headings': '#6b6b88',
            '--tw-prose-code': '#6b6b88',
            '--tw-prose-links': '#6b6b88',
            '--tw-prose-quotes': '#6b6b88',
            '--tw-prose-kbd': '#6b6b88',
            '--tw-prose-pre-code': '#6b6b88',
            '--tw-prose-pre-bg': '#14141e',
          } as React.CSSProperties}
        >
          <div className="prose prose-invert max-w-none" style={{ fontSize: '11px' }}>
            <MarkdownRenderer>{block.content}</MarkdownRenderer>
          </div>
        </div>
      )}
    </div>
  )
}

// ── Tool row ──────────────────────────────────────────────────────────────────

function ToolRow({ block }: { block: ToolBlock }) {
  const [open, setOpen] = useState(false)

  return (
    <div className="flex flex-col">
      <div
        className="flex items-center gap-2 py-[5px] pr-1 cursor-pointer"
        onClick={() => setOpen(o => !o)}
      >
        <div className="w-[18px] h-[18px] flex items-center justify-center rounded bg-background border border-[var(--secondary)] shrink-0">
          <Wrench size={9} className="text-[var(--primary)]" style={{ opacity: 0.7 }} />
        </div>

        <span className="text-[11px] text-primary opacity-65 flex-1 truncate font-mono">{block.name}</span>

        {!block.done && (
          <span className="flex gap-0.5 shrink-0">
            <span className="w-1 h-1 rounded-full bg-[var(--primary)] animate-bounce" style={{ animationDelay: '0ms' }} />
            <span className="w-1 h-1 rounded-full bg-[var(--primary)] animate-bounce" style={{ animationDelay: '150ms' }} />
            <span className="w-1 h-1 rounded-full bg-[var(--primary)] animate-bounce" style={{ animationDelay: '300ms' }} />
          </span>
        )}

        <ChevronRight
          size={10}
          className={cn('text-[var(--accent)] shrink-0 transition-transform', open && 'rotate-90')}
        />
      </div>

      {open && (
        <div className="ml-[26px] mb-[6px] px-3 py-2.5 bg-background border border-[var(--secondary)] rounded-lg space-y-2">
          <div>
            <p className="text-[10px] text-[var(--accent)] uppercase tracking-wider mb-1">Input</p>
            <pre className="text-[11px] text-muted-foreground/70 font-mono whitespace-pre-wrap break-all leading-relaxed">
              {JSON.stringify(block.args, null, 2)}
            </pre>
          </div>
          {block.result != null && (
            <div>
              <p className="text-[10px] text-[var(--accent)] uppercase tracking-wider mb-1">Output</p>
              <pre className="text-[11px] text-muted-foreground/70 font-mono whitespace-pre-wrap break-all leading-relaxed max-h-48 overflow-y-auto">
                {block.result}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

// ── Skill row ─────────────────────────────────────────────────────────────────

function SkillRow({ block }: { block: SkillBlock }) {
  return (
    <div className="flex items-center gap-2 py-[5px] pr-1">
      <div className="w-[18px] h-[18px] flex items-center justify-center rounded bg-background border border-[var(--secondary)] shrink-0">
        <Zap size={9} className="text-primary/80" style={{ opacity: 0.8 }} />
      </div>
      <span className="text-[11px] text-primary/80 opacity-65 flex-1 truncate font-mono">{block.name}</span>
    </div>
  )
}

// ── TimelineStrip ─────────────────────────────────────────────────────────────

interface Props {
  blocks: StreamBlock[]
  resolvedPermissions?: ResolvedPermission[]
  pendingPermission?: PermissionRequest | null
}

export function TimelineStrip({ blocks, resolvedPermissions = [], pendingPermission }: Props) {
  if (blocks.length === 0 && resolvedPermissions.length === 0 && !pendingPermission) return null
  let toolIdx = 0
  return (
    <div className="w-full border-l-2 border-[var(--secondary)] pl-3 mb-[6px]">
      {blocks.map((block, i) => {
        if (block.type === 'tool') {
          const perm = resolvedPermissions[toolIdx]
          toolIdx++
          return (
            <div key={i}>
              {perm && <div className="mb-1"><ResolvedBadge resolved={perm} /></div>}
              <ToolRow block={block} />
            </div>
          )
        }
        return block.type === 'thinking' ? <ThinkingRow key={i} block={block} /> : <SkillRow key={i} block={block} />
      })}
      {pendingPermission && <div className="mt-1"><PendingBubble req={pendingPermission} /></div>}
    </div>
  )
}
