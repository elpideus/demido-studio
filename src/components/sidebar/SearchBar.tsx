import { Search } from 'lucide-react'
import { useState } from 'react'
import DOMPurify from 'dompurify'
import { db } from '../../lib/tauri'
import { useConversations } from '../../stores/conversations'

export function SearchBar() {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<{ conversation_id: string; snippet: string }[]>([])
  const { setActive } = useConversations()

  const handleSearch = async (q: string) => {
    setQuery(q)
    if (q.trim().length < 2) { setResults([]); return }
    const r = await db.searchConversations(q.trim())
    setResults(r)
  }

  return (
    <div className="px-2 pt-2 pb-1 relative">
      <div className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-secondary border border-border">
        <Search size={13} className="text-muted-foreground shrink-0" />
        <input
          value={query}
          onChange={e => handleSearch(e.target.value)}
          placeholder="Search..."
          className="bg-transparent text-sm text-foreground placeholder:text-muted-foreground outline-none flex-1 w-0"
        />
      </div>
      {results.length > 0 && (
        <div className="absolute left-2 right-2 top-full mt-1 bg-secondary border border-border rounded-md shadow-lg z-10 max-h-64 overflow-y-auto">
          {results.map((r, i) => (
            <button
              key={i}
              className="w-full text-left px-3 py-2 text-xs text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
              onClick={() => { setActive(r.conversation_id); setResults([]); setQuery('') }}
              dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(r.snippet, { ALLOWED_TAGS: ['mark'] }) }}
            />
          ))}
        </div>
      )}
    </div>
  )
}
