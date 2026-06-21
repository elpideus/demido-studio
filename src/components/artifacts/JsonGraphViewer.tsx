import { memo, useMemo, useRef, useCallback, useState } from 'react'
import JSON5 from 'json5'
import {
  ReactFlow,
  Background,
  Controls,
  ControlButton,
  MiniMap,
  useReactFlow,
  type Node,
  type Edge,
  BackgroundVariant,
  Handle,
  Position,
  type NodeMouseHandler,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'

type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue }
type Dir = 'LR' | 'TB'

const PALETTE = {
  object:  { bg: '#252840', text: '#a5b4fc' },
  array:   { bg: '#2a2040', text: '#c4b5fd' },
  string:  { bg: '#1a2e28', text: '#86efac' },
  number:  { bg: '#1a2436', text: '#93c5fd' },
  boolean: { bg: '#2e2515', text: '#fcd34d' },
  null:    { bg: '#2e1515', text: '#fca5a5' },
  index:   { bg: '#1e1e28', text: '#6b7280' },
}

const X_GAP = 260
const Y_GAP = 90
const MAX_ARRAY_ITEMS = 30
const MAX_DEPTH = 8

const JsonValueNode = memo(function JsonValueNode({ data }: { data: { label: string; kind: string; value?: string; dir?: Dir } }) {
  const p = PALETTE[data.kind as keyof typeof PALETTE] ?? PALETTE.null
  const isLR = (data.dir ?? 'LR') === 'LR'
  return (
    <div style={{ background: p.bg, color: p.text, borderRadius: 8, padding: '6px 10px', fontSize: 11, fontFamily: 'monospace', minWidth: 80, maxWidth: 220, userSelect: 'none' }}>
      <Handle type="target" position={isLR ? Position.Left : Position.Top} style={{ background: p.text, width: 7, height: 7, border: 'none' }} />
      <div style={{ color: '#9ca3af', fontSize: 10, marginBottom: data.value !== undefined ? 2 : 0 }}>{data.label}</div>
      {data.value !== undefined && (
        <div style={{ color: p.text, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{data.value}</div>
      )}
      <Handle type="source" position={isLR ? Position.Right : Position.Bottom} style={{ background: p.text, width: 7, height: 7, border: 'none' }} />
    </div>
  )
})

const JsonIndexNode = memo(function JsonIndexNode({ data }: { data: { index: string; dir?: Dir } }) {
  const p = PALETTE.index
  const isLR = (data.dir ?? 'LR') === 'LR'
  return (
    <div style={{ background: p.bg, color: p.text, borderRadius: 99, padding: '2px 8px', fontSize: 10, fontFamily: 'monospace', userSelect: 'none', whiteSpace: 'nowrap', opacity: 0.8 }}>
      <Handle type="target" position={isLR ? Position.Left : Position.Top} style={{ background: p.text, width: 6, height: 6, border: 'none' }} />
      [{data.index}]
      <Handle type="source" position={isLR ? Position.Right : Position.Bottom} style={{ background: p.text, width: 6, height: 6, border: 'none' }} />
    </div>
  )
})

const nodeTypes = { jsonValue: JsonValueNode, jsonIndex: JsonIndexNode }

const MINIMAP_H = 140

function NavMiniMap({ strokeWidth }: { strokeWidth: number }) {
  const { setCenter } = useReactFlow()
  return (
    <MiniMap
      nodeColor={n => {
        if (n.type === 'jsonIndex') return PALETTE.index.bg
        return PALETTE[(n.data as { kind?: string }).kind as keyof typeof PALETTE]?.bg ?? PALETTE.null.bg
      }}
      nodeStrokeColor={n => {
        if (n.type === 'jsonIndex') return PALETTE.index.bg
        return PALETTE[(n.data as { kind?: string }).kind as keyof typeof PALETTE]?.bg ?? PALETTE.null.bg
      }}
      nodeStrokeWidth={strokeWidth}
      maskColor="rgba(0,0,0,0.4)"
      style={{ background: '#0d0d14', border: '1px solid #374151' }}
      zoomable
      pannable
      onClick={(_, pos) => setCenter(pos.x, pos.y, { duration: 200 })}
    />
  )
}

function buildGraph(
  value: JsonValue,
  nodeId: string,
  label: string,
  nodes: Node[],
  edges: Edge[],
  depth: number,
  siblingIndex: number,
  dir: Dir,
): { height: number; center: number } {
  const isArr = Array.isArray(value)
  const isObj = value !== null && typeof value === 'object'
  const kind = isArr ? 'array' : isObj ? 'object' : value === null ? 'null' : typeof value

  const pos = (d: number, s: number) => dir === 'LR'
    ? { x: d * X_GAP, y: s * Y_GAP }
    : { x: s * (Y_GAP * 3.2), y: d * (X_GAP * 0.48) }

  if (!isObj) {
    const displayVal = value === null ? 'null' : typeof value === 'string' ? `"${value}"` : String(value)
    nodes.push({ id: nodeId, type: 'jsonValue', position: pos(depth, siblingIndex), data: { label, kind, value: displayVal, dir }, initialWidth: 150, initialHeight: 46 })
    return { height: 1, center: siblingIndex }
  }

  if (depth >= MAX_DEPTH) {
    const summary = isArr ? `${(value as JsonValue[]).length} items` : `${Object.keys(value as object).length} keys`
    nodes.push({ id: nodeId, type: 'jsonValue', position: pos(depth, siblingIndex), data: { label, kind, value: summary, dir }, initialWidth: 150, initialHeight: 46 })
    return { height: 1, center: siblingIndex }
  }

  const allEntries: [string, JsonValue][] = isArr
    ? (value as JsonValue[]).map((v, i) => [String(i), v])
    : Object.entries(value as Record<string, JsonValue>)

  const truncated = isArr && allEntries.length > MAX_ARRAY_ITEMS
  const entries = truncated ? allEntries.slice(0, MAX_ARRAY_ITEMS) : allEntries

  const childCenters: number[] = []
  let childOffset = siblingIndex
  for (const [k, v] of entries) {
    const isComplexArrayItem = isArr && v !== null && typeof v === 'object'
    if (isComplexArrayItem) {
      const grandchildren: [string, JsonValue][] = Array.isArray(v)
        ? (v as JsonValue[]).map((gv, gi) => [String(gi), gv])
        : Object.entries(v as Record<string, JsonValue>)
      const indexId = `${nodeId}__idx${k}`
      edges.push({ id: `e-${nodeId}-${indexId}`, source: nodeId, target: indexId, style: { stroke: '#374151', strokeWidth: 1 } })
      let gcOffset = childOffset
      const gcCenters: number[] = []
      for (const [gk, gv] of grandchildren) {
        const gcId = `${nodeId}__${k}__${gk}`
        edges.push({ id: `e-${indexId}-${gcId}`, source: indexId, target: gcId, style: { stroke: '#374151', strokeWidth: 1 } })
        const { height, center } = buildGraph(gv, gcId, gk, nodes, edges, depth + 2, gcOffset, dir)
        gcCenters.push(center)
        gcOffset += height
      }
      const centerY = gcCenters.length ? (gcCenters[0] + gcCenters[gcCenters.length - 1]) / 2 : childOffset
      nodes.push({ id: indexId, type: 'jsonIndex', position: pos(depth + 1, centerY), data: { index: k, dir }, initialWidth: 52, initialHeight: 22 })
      childCenters.push(centerY)
      childOffset = Math.max(childOffset + 1, gcOffset)
    } else {
      const childId = `${nodeId}__${k}`
      edges.push({ id: `e-${nodeId}-${childId}`, source: nodeId, target: childId, style: { stroke: '#374151', strokeWidth: 1 } })
      const { height, center } = buildGraph(v, childId, isArr ? `[${k}]` : k, nodes, edges, depth + 1, childOffset, dir)
      childCenters.push(center)
      childOffset += height
    }
  }

  if (truncated) {
    const moreId = `${nodeId}__more`
    const remaining = allEntries.length - MAX_ARRAY_ITEMS
    nodes.push({ id: moreId, type: 'jsonValue', position: pos(depth + 1, childOffset), data: { label: '…', kind: 'null', value: `${remaining} more items`, dir }, initialWidth: 150, initialHeight: 46 })
    edges.push({ id: `e-${nodeId}-${moreId}`, source: nodeId, target: moreId, style: { stroke: '#374151', strokeWidth: 1 } })
    childCenters.push(childOffset)
    childOffset += 1
  }

  const center = childCenters.length ? (childCenters[0] + childCenters[childCenters.length - 1]) / 2 : siblingIndex
  nodes.push({ id: nodeId, type: 'jsonValue', position: pos(depth, center), data: { label, kind, dir }, initialWidth: 150, initialHeight: 46 })

  return { height: Math.max(1, childOffset - siblingIndex), center }
}

export function JsonGraphViewer({ content }: { content: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [dir, setDir] = useState<Dir>('LR')

  let parsed: JsonValue
  try {
    parsed = JSON5.parse(content)
  } catch {
    return <div className="p-4 text-red-400 text-sm font-mono">Invalid JSON</div>
  }

  const { nodes, edges, adjacency, minimapStrokeWidth } = useMemo(() => {
    const nodes: Node[] = []
    const edges: Edge[] = []
    buildGraph(parsed, 'root', 'root', nodes, edges, 0, 0, dir)
    const adjacency = new Map<string, Set<string>>()
    for (const e of edges) {
      if (!adjacency.has(e.source)) adjacency.set(e.source, new Set())
      if (!adjacency.has(e.target)) adjacency.set(e.target, new Set())
      adjacency.get(e.source)!.add(e.target)
      adjacency.get(e.target)!.add(e.source)
    }
    // compute stroke so each node appears >= 5px tall in MINIMAP_H-px minimap
    const ys = nodes.map(n => n.position.y)
    const graphH = nodes.length > 1 ? Math.max(...ys) - Math.min(...ys) + Y_GAP : Y_GAP
    const minimapStrokeWidth = Math.max(Y_GAP / 2, (5 * graphH / MINIMAP_H - 50) / 2)
    return { nodes, edges, adjacency, minimapStrokeWidth }
  }, [content, dir])

  const onNodeMouseEnter: NodeMouseHandler = useCallback((_, node) => {
    const root = containerRef.current
    if (!root) return
    const connected = new Set<string>([node.id])
    const queue = [node.id]
    while (queue.length) {
      const cur = queue.shift()!
      for (const nb of adjacency.get(cur) ?? []) {
        if (!connected.has(nb)) { connected.add(nb); queue.push(nb) }
      }
    }
    const connectedEdges = new Set(edges.filter(e => connected.has(e.source) && connected.has(e.target)).map(e => e.id))
    root.querySelectorAll<HTMLElement>('.react-flow__node').forEach(el => {
      el.style.opacity = connected.has(el.getAttribute('data-id') ?? '') ? '1' : '0.1'
      el.style.transition = 'opacity 0.12s'
    })
    root.querySelectorAll<HTMLElement>('.react-flow__edge').forEach(el => {
      el.style.opacity = connectedEdges.has(el.getAttribute('data-id') ?? '') ? '1' : '0.05'
      el.style.transition = 'opacity 0.12s'
    })
  }, [adjacency, edges])

  const onNodeMouseLeave: NodeMouseHandler = useCallback(() => {
    const root = containerRef.current
    if (!root) return
    root.querySelectorAll<HTMLElement>('.react-flow__node, .react-flow__edge').forEach(el => { el.style.opacity = '1' })
  }, [])

  return (
    <div ref={containerRef} className="w-full h-full relative">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodeMouseEnter={onNodeMouseEnter}
        onNodeMouseLeave={onNodeMouseLeave}
        fitView
        fitViewOptions={{ padding: 0.15 }}
        minZoom={0.001}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        proOptions={{ hideAttribution: true }}
      >
        <Background variant={BackgroundVariant.Dots} gap={32} size={2} color="#ffffff18" />
        <Controls showInteractive={false} className="[&_button]:!bg-[#1b1b1b] [&_button]:!border-[#374151] [&_button]:!text-[#9ca3af] [&_button:hover]:!bg-[#252525]">
          <ControlButton
            onClick={() => setDir(d => d === 'LR' ? 'TB' : 'LR')}
            title={dir === 'LR' ? 'Switch to vertical layout' : 'Switch to horizontal layout'}
          >
            {dir === 'LR' ? (
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" width="12" height="12">
                <path d="M8 2v12M4 10l4 4 4-4" />
                <path d="M2 5h12" strokeOpacity="0.4" />
              </svg>
            ) : (
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" width="12" height="12">
                <path d="M2 8h12M10 4l4 4-4 4" />
                <path d="M5 2v12" strokeOpacity="0.4" />
              </svg>
            )}
          </ControlButton>
        </Controls>
        <NavMiniMap strokeWidth={minimapStrokeWidth} />
      </ReactFlow>
    </div>
  )
}
