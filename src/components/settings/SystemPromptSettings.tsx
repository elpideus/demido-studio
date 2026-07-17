import { useEffect, useRef, useState } from 'react'
import { systemPrompt } from '../../lib/tauri'

type Status = 'idle' | 'saving' | 'saved' | 'error'

/**
 * Editor over `system_prompt.md` in app-data — the file is the source of truth, so this panel is a
 * view onto it, not a settings row. Reloaded on mount because the user may have edited the file
 * outside the app since the panel last rendered.
 */
export function SystemPromptSettings() {
  const [text, setText] = useState('')
  const [path, setPath] = useState('')
  const [vars, setVars] = useState<string[]>([])
  const [status, setStatus] = useState<Status>('idle')
  const [error, setError] = useState('')
  const areaRef = useRef<HTMLTextAreaElement>(null)
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined)

  useEffect(() => {
    Promise.all([systemPrompt.get(), systemPrompt.path(), systemPrompt.listVars()])
      .then(([content, p, v]) => { setText(content); setPath(p); setVars(v) })
      .catch(err => { setStatus('error'); setError(String(err)) })
    return () => clearTimeout(saveTimer.current)
  }, [])

  const save = (content: string) => {
    setStatus('saving')
    systemPrompt.set(content)
      .then(() => { setStatus('saved'); setError('') })
      .catch(err => { setStatus('error'); setError(String(err)) })
  }

  const onChange = (content: string) => {
    setText(content)
    clearTimeout(saveTimer.current)
    saveTimer.current = setTimeout(() => save(content), 600)
  }

  // Insert at the caret rather than appending: prompts are long, and the var almost always belongs
  // in the sentence being written.
  const insertVar = (name: string) => {
    const area = areaRef.current
    const token = '${' + name + '}'
    const at = area?.selectionStart ?? text.length
    const next = text.slice(0, at) + token + text.slice(area?.selectionEnd ?? at)
    onChange(next)
    requestAnimationFrame(() => {
      area?.focus()
      area?.setSelectionRange(at + token.length, at + token.length)
    })
  }

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-semibold text-foreground">Global System Prompt</h3>
        <p className="text-xs text-muted-foreground mt-1">
          Prepended to every conversation. Stored as a Markdown file — edit it here or in any editor;
          changes apply to the next message.
        </p>
      </div>

      <textarea
        ref={areaRef}
        value={text}
        onChange={e => onChange(e.target.value)}
        onBlur={() => { clearTimeout(saveTimer.current); save(text) }}
        placeholder="You are a helpful assistant..."
        rows={14}
        spellCheck={false}
        className="w-full bg-background border border-border rounded-xl px-4 py-3 text-sm text-foreground placeholder:text-muted-foreground outline-none focus:border-ring/50 resize-none leading-relaxed font-mono"
      />

      <div className="flex items-center justify-between gap-3">
        <code className="text-[11px] text-muted-foreground truncate" title={path}>{path}</code>
        <span className="text-xs shrink-0 text-muted-foreground">
          {status === 'saving' && 'Saving…'}
          {status === 'saved'  && 'Saved'}
          {status === 'error'  && <span className="text-red-400">{error}</span>}
        </span>
      </div>

      <div className="space-y-2 pt-2 border-t border-border">
        <p className="text-xs text-muted-foreground">
          Variables — substituted when the prompt is sent. Also work in skills. Write{' '}
          <code className="text-foreground">{'\\${NAME}'}</code> to keep one literal.
        </p>
        <div className="flex flex-wrap gap-1.5">
          {vars.map(v => (
            <button
              key={v}
              onClick={() => insertVar(v)}
              className="px-2 py-1 rounded-md bg-accent/60 hover:bg-accent text-[11px] font-mono text-muted-foreground hover:text-foreground transition-colors"
            >
              {'${' + v + '}'}
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}
