import { useState, useEffect, useMemo, useRef, useCallback } from 'react'
import { ChevronRight, ChevronDown, Folder, FolderOpen, Search, X } from 'lucide-react'
import * as fileIconsJs from 'file-icons-js'
import 'file-icons-js/css/style.css'
import { clampToViewport } from '../../lib/utils'
import { fs } from '../../lib/tauri'
import { useArtifacts } from '../../stores/artifacts'
import { useImageEditor } from '../../stores/imageEditor'
import { useWindowManager } from '../../stores/windowManager'
import type { FsEntry } from '../../types'

const IMAGE_EXTS = new Set(['png','jpg','jpeg','gif','webp','bmp','svg','tiff','tif','ico','avif'])
function isImage(name: string): boolean {
  return IMAGE_EXTS.has(name.split('.').pop()?.toLowerCase() ?? '')
}

interface TreeNode extends FsEntry {
  children: TreeNode[] | null
  expanded: boolean
}

function FileTypeIcon({ name }: { name: string }) {
  const cls = fileIconsJs.getClassWithColor(name) ?? 'default-icon'
  return <span className={`${cls} shrink-0`} style={{ fontSize: 12, lineHeight: 1, display: 'inline-block', width: 12 }} />
}

function fileType(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  const m: Record<string, string> = {
    ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    rs: 'rust', py: 'python', json: 'json', md: 'markdown', html: 'html',
    css: 'css', toml: 'toml', yaml: 'yaml', yml: 'yaml', sh: 'bash',
    txt: 'text', sql: 'sql', go: 'go', java: 'java', cpp: 'cpp', c: 'c',
  }
  return (m[ext] ?? ext) || 'text'
}

const EDITABLE_EXTS = new Set(['ts','tsx','js','jsx','rs','py','go','java','cpp','c','cs','json','toml','yaml','yml','md','html','css','scss','sh','bash','sql','txt','env','gitignore'])
function isEditable(name: string): boolean {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  return EDITABLE_EXTS.has(ext)
}

function toNodes(entries: FsEntry[]): TreeNode[] {
  return entries.map(e => ({ ...e, children: e.isDir ? null : (undefined as any), expanded: false }))
}

function updateNode(nodes: TreeNode[], path: string, up: (n: TreeNode) => TreeNode): TreeNode[] {
  return nodes.map(n => {
    if (n.path === path) return up(n)
    if (n.children) return { ...n, children: updateNode(n.children, path, up) }
    return n
  })
}

function removeNode(nodes: TreeNode[], path: string): TreeNode[] {
  return nodes.filter(n => n.path !== path).map(n =>
    n.children ? { ...n, children: removeNode(n.children, path) } : n
  )
}

function findNode(nodes: TreeNode[], path: string): TreeNode | undefined {
  for (const n of nodes) {
    if (n.path === path) return n
    if (n.children) { const f = findNode(n.children, path); if (f) return f }
  }
}

function flattenNodes(nodes: TreeNode[]): TreeNode[] {
  const result: TreeNode[] = []
  for (const n of nodes) {
    result.push(n)
    if (n.children) result.push(...flattenNodes(n.children))
  }
  return result
}

function toRel(absPath: string, rootPath: string): string {
  const norm = (p: string) => p.replace(/\\/g, '/').replace(/\/+$/, '')
  const base = norm(rootPath)
  const path = norm(absPath)
  if (path.toLowerCase().startsWith(base.toLowerCase() + '/')) return path.slice(base.length + 1)
  return path
}

interface CtxMenu { x: number; y: number; node: TreeNode }

interface NodeRowProps {
  node: TreeNode
  depth: number
  rootPath: string
  renamingPath: string | null
  onToggle: (path: string) => void
  onOpen: (node: TreeNode) => void
  onCtxMenu: (e: React.MouseEvent, node: TreeNode) => void
  onRenameCommit: (node: TreeNode, newName: string) => void
  onRenameCancel: () => void
}

function NodeRow({ node, depth, rootPath, renamingPath, onToggle, onOpen, onCtxMenu, onRenameCommit, onRenameCancel }: NodeRowProps) {
  const pl = depth * 12 + 6
  const renaming = renamingPath === node.path
  const [draft, setDraft] = useState(node.name)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (renaming) { setDraft(node.name); setTimeout(() => inputRef.current?.select(), 0) }
  }, [renaming, node.name])

  const onDragStart = (e: React.DragEvent) => {
    e.dataTransfer.setData('text/plain', toRel(node.path, rootPath))
    e.dataTransfer.effectAllowed = 'copy'
  }

  const nameEl = renaming ? (
    <input
      ref={inputRef}
      value={draft}
      onChange={e => setDraft(e.target.value)}
      onKeyDown={e => {
        if (e.key === 'Enter') onRenameCommit(node, draft)
        if (e.key === 'Escape') onRenameCancel()
      }}
      onBlur={() => onRenameCancel()}
      onClick={e => e.stopPropagation()}
      className="flex-1 min-w-0 bg-accent border border-border rounded px-1 text-[11px] text-foreground outline-none"
      autoFocus
    />
  ) : null

  if (node.isDir) {
    return (
      <>
        <div
          style={{ paddingLeft: pl }}
          className="flex items-center gap-1 py-[3px] pr-2 cursor-pointer hover:bg-accent/50 select-none group"
          onClick={() => !renaming && onToggle(node.path)}
          onContextMenu={e => onCtxMenu(e, node)}
          draggable={!renaming}
          onDragStart={onDragStart}
        >
          <span className="w-3 shrink-0 flex text-muted-foreground/60">
            {node.expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          </span>
          {node.expanded
            ? <FolderOpen size={12} className="shrink-0" style={{ color: '#e8b84b' }} />
            : <Folder size={12} className="shrink-0" style={{ color: '#e8b84b' }} />
          }
          {nameEl ?? <span className="ml-1 truncate text-[11px] text-foreground/75">{node.name}</span>}
        </div>
        {node.expanded && node.children && node.children.map(c =>
          <NodeRow key={c.path} node={c} depth={depth + 1} rootPath={rootPath} renamingPath={renamingPath}
            onToggle={onToggle} onOpen={onOpen} onCtxMenu={onCtxMenu} onRenameCommit={onRenameCommit} onRenameCancel={onRenameCancel} />
        )}
      </>
    )
  }

  return (
    <div
      style={{ paddingLeft: pl + 14 }}
      className="flex items-center gap-1 py-[3px] pr-2 cursor-pointer hover:bg-accent/50 select-none"
      onClick={() => !renaming && onOpen(node)}
      onContextMenu={e => onCtxMenu(e, node)}
      draggable={!renaming}
      onDragStart={onDragStart}
    >
      <FileTypeIcon name={node.name} />
      {nameEl ?? <span className="ml-1 truncate text-[11px] text-foreground/60">{node.name}</span>}
    </div>
  )
}

export function FileExplorer({ rootPath, conversationId }: { rootPath: string; conversationId: string }) {
  const [nodes, setNodes] = useState<TreeNode[]>([])
  const [error, setError] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)
  const [copiedPath, setCopiedPath] = useState<string | null>(null)
  const { setActive } = useArtifacts()
  const { openWithImage } = useImageEditor()
  const { openWindow } = useWindowManager()
  const ctxMenuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    setNodes([]); setError(null)
    fs.listDir(conversationId, rootPath)
      .then(entries => setNodes(toNodes(entries)))
      .catch(e => setError(String(e)))
  }, [rootPath, conversationId])

  // dismiss context menu on outside click
  useEffect(() => {
    if (!ctxMenu) return
    requestAnimationFrame(() => ctxMenuRef.current && clampToViewport(ctxMenuRef.current))
    const dismiss = () => setCtxMenu(null)
    window.addEventListener('mousedown', dismiss)
    return () => window.removeEventListener('mousedown', dismiss)
  }, [ctxMenu])

  const reload = useCallback(async (dirPath: string) => {
    try {
      const entries = await fs.listDir(conversationId, dirPath)
      setNodes(prev => dirPath === rootPath
        ? toNodes(entries)
        : updateNode(prev, dirPath, n => ({ ...n, children: toNodes(entries) }))
      )
    } catch { /* ignore */ }
  }, [conversationId, rootPath])

  const toggle = async (path: string) => {
    const node = findNode(nodes, path)
    if (!node?.isDir) return
    if (node.expanded) {
      setNodes(prev => updateNode(prev, path, n => ({ ...n, expanded: false })))
      return
    }
    if (node.children === null) {
      try {
        const entries = await fs.listDir(conversationId, path)
        setNodes(prev => updateNode(prev, path, n => ({ ...n, expanded: true, children: toNodes(entries) })))
      } catch {
        setNodes(prev => updateNode(prev, path, n => ({ ...n, expanded: true, children: [] })))
      }
    } else {
      setNodes(prev => updateNode(prev, path, n => ({ ...n, expanded: true })))
    }
  }

  const openFile = async (node: TreeNode) => {
    const ext = node.name.split('.').pop()?.toLowerCase()
    try {
      if (isImage(node.name)) {
        const b64 = await fs.readFileBase64(conversationId, node.path)
        const mime = ext === 'svg' ? 'image/svg+xml' : ext === 'jpg' || ext === 'jpeg' ? 'image/jpeg' : `image/${ext}`
        const dataUrl = `data:${mime};base64,${b64}`
        openWithImage(dataUrl, node.name)
        openWindow('image-editor', 'image-editor', node.name, { initialSize: { width: 1100, height: 720 } })
      } else if (ext === 'pdf') {
        const b64 = await fs.readFileBase64(conversationId, node.path)
        setActive({ id: `__explorer__:${node.path}`, messageId: '__explorer__', type: 'pdf', title: node.name, content: `data:application/pdf;base64,${b64}` })
      } else {
        const content = await fs.readFile(conversationId, node.path)
        setActive({ id: `__explorer__:${node.path}`, messageId: '__explorer__', type: fileType(node.name), title: node.name, content })
      }
    } catch { /* binary/large files silently ignored */ }
  }

  const handleCtxMenu = (e: React.MouseEvent, node: TreeNode) => {
    e.preventDefault()
    e.stopPropagation()
    setCtxMenu({ x: e.clientX, y: e.clientY, node })
  }

  const handleRenameCommit = async (node: TreeNode, newName: string) => {
    setRenamingPath(null)
    if (!newName.trim() || newName === node.name) return
    try {
      await fs.rename(conversationId, node.path, newName.trim())
      const parent = node.path.replace(/\\/g, '/').split('/').slice(0, -1).join('/')
      await reload(parent || rootPath)
    } catch (e) { setError(String(e)) }
  }

  const handleDelete = async (node: TreeNode) => {
    setCtxMenu(null)
    try {
      await fs.delete(conversationId, node.path)
      setNodes(prev => removeNode(prev, node.path))
    } catch (e) { setError(String(e)) }
  }

  const handlePaste = async (targetNode: TreeNode) => {
    if (!copiedPath) return
    setCtxMenu(null)
    const destDir = targetNode.isDir ? targetNode.path : rootPath
    try {
      await fs.copyDir(conversationId, copiedPath, destDir)
      await reload(destDir)
    } catch (e) { setError(String(e)) }
  }

  const searchResults = useMemo(() => {
    if (!query.trim()) return null
    const q = query.toLowerCase()
    return flattenNodes(nodes).filter(n => !n.isDir && n.name.toLowerCase().includes(q))
  }, [query, nodes])

  if (error) return <p className="text-[10px] text-red-400 px-3 py-2 truncate">{error}</p>

  return (
    <div className="flex flex-col flex-1 min-h-0">
      <div className="flex items-center gap-1 px-2 py-1.5 border-b border-border/40">
        <Search size={10} className="text-muted-foreground/50 shrink-0" />
        <input
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="Search files…"
          className="flex-1 bg-transparent text-[11px] text-foreground/75 placeholder:text-muted-foreground/40 outline-none min-w-0"
        />
        {query && (
          <button onClick={() => setQuery('')} className="text-muted-foreground/50 hover:text-foreground/75">
            <X size={10} />
          </button>
        )}
      </div>

      <div className="flex-1 overflow-y-auto overflow-x-hidden">
        {searchResults ? (
          searchResults.length === 0
            ? <p className="text-[10px] text-muted-foreground px-3 py-2">No results</p>
            : searchResults.map(n => (
                <div
                  key={n.path}
                  className="flex items-center gap-1 py-[3px] px-2 cursor-pointer hover:bg-accent/50 select-none"
                  onClick={() => openFile(n)}
                  onContextMenu={e => handleCtxMenu(e, n)}
                  draggable
                  onDragStart={e => { e.dataTransfer.setData('text/plain', toRel(n.path, rootPath)); e.dataTransfer.effectAllowed = 'copy' }}
                >
                  <FileTypeIcon name={n.name} />
                  <span className="ml-1 truncate text-[11px] text-foreground/60">{n.name}</span>
                  <span className="ml-auto truncate text-[10px] text-muted-foreground/40 max-w-[60%] text-right">
                    {toRel(n.path, rootPath).split('/').slice(0, -1).join('/')}
                  </span>
                </div>
              ))
        ) : nodes.length === 0
          ? <p className="text-[10px] text-muted-foreground px-3 py-2">Empty</p>
          : nodes.map(n =>
              <NodeRow key={n.path} node={n} depth={0} rootPath={rootPath} renamingPath={renamingPath}
                onToggle={toggle} onOpen={openFile} onCtxMenu={handleCtxMenu}
                onRenameCommit={handleRenameCommit} onRenameCancel={() => setRenamingPath(null)} />
            )
        }
      </div>

      {ctxMenu && (
        <div
          ref={ctxMenuRef}
          className="fixed z-50 min-w-[140px] bg-popover border border-border rounded-md shadow-md py-1 text-[12px]"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={e => e.stopPropagation()}
        >
          {!ctxMenu.node.isDir && (
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
              onClick={() => { openFile(ctxMenu.node); setCtxMenu(null) }}
            >
              {isEditable(ctxMenu.node.name) ? 'Edit' : 'Open'}
            </button>
          )}
          {ctxMenu.node.isDir && (
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
              onClick={() => { toggle(ctxMenu.node.path); setCtxMenu(null) }}
            >
              Open
            </button>
          )}
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
            onClick={() => { setRenamingPath(ctxMenu.node.path); setCtxMenu(null) }}
          >
            Rename
          </button>
          {ctxMenu.node.isDir && (
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
              onClick={() => { setCopiedPath(ctxMenu.node.path); setCtxMenu(null) }}
            >
              Copy{copiedPath === ctxMenu.node.path ? ' ✓' : ''}
            </button>
          )}
          {ctxMenu.node.isDir && copiedPath && copiedPath !== ctxMenu.node.path && (
            <button
              className="w-full text-left px-3 py-1.5 hover:bg-accent/60 text-foreground/80"
              onClick={() => handlePaste(ctxMenu.node)}
            >
              Paste here
            </button>
          )}
          <div className="border-t border-border/40 my-1" />
          <button
            className="w-full text-left px-3 py-1.5 hover:bg-red-500/20 text-red-400"
            onClick={() => handleDelete(ctxMenu.node)}
          >
            Delete
          </button>
        </div>
      )}
    </div>
  )
}
