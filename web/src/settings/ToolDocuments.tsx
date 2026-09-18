import { useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, Lock, RotateCcw } from 'lucide-react'

import { useDraft } from './Control'
import { Depends, Moved } from './Prompts'
import { outdated, useToolDocuments, type Shape, type ToolDocument } from './register'
import styles from './Prompts.module.css'

/**
 * The tool register's editor: one document per host tool, each opening into
 * its text.
 *
 * The brief's
 *
 * > All prompts should be editable.
 *
 * for the text that matters most: the words a model picks a tool on, sent on
 * every turn the tool is switched on (`docs/rules/prompts.md`,
 * [#77](https://github.com/elpideus/demido-studio/issues/77)).
 *
 * **A tool is one document.** Its description and its parameter prose are one
 * field, edited together and hashed together, because one hash covers
 * everything about the tool the model reads and a field per parameter would be
 * a page implying they version apart. **The shape is not in the field.** It is
 * drawn above it, as what it is: the names, types and which are required, a
 * contract with the parser and never text.
 *
 * Everything else is the paragraph editor's, reused rather than redrawn: the
 * dependants above the field, the diff and the reset when the shipped wording
 * has moved, and the way back to the default. Both registers therefore say the
 * same thing about what an edit costs.
 */
export function ToolDocumentsPage() {
  const entries = useToolDocuments((documents) => documents.entries)
  const read = useToolDocuments((documents) => documents.read)
  const [open, setOpen] = useState<string | null>(null)

  useEffect(() => {
    void read()
  }, [read])

  if (!entries) return <div className={styles.rows} />

  return (
    <div className={styles.rows}>
      {entries.map((document) => (
        <ToolDocumentControl
          key={document.tool.name}
          document={document}
          open={open === document.tool.name}
          toggle={() => setOpen(open === document.tool.name ? null : document.tool.name)}
        />
      ))}
    </div>
  )
}

/**
 * One tool's document: its name, and what the model is told about it.
 *
 * Exported as **the** control for a tool document. `docs/rules/setup.md`'s
 * one-component rule: a set-up step that touches an entry renders this, and not
 * a second drawing of it. No step does yet.
 */
export function ToolDocumentControl({
  document,
  open,
  toggle,
}: {
  document: ToolDocument
  open: boolean
  toggle: () => void
}) {
  const { tool } = document
  const set = useToolDocuments((documents) => documents.set)
  const reset = useToolDocuments((documents) => documents.reset)
  const edited = document.origin === 'edited'
  const [draft, setDraft, commit] = useDraft(document.text, async (text) =>
    text === document.text ? true : set(tool.name, text),
  )

  const gone = new Set(document.suppressed.map((dependant) => dependant.note))
  const Chevron = open ? ChevronDown : ChevronRight

  return (
    <article className={styles.entry} data-edited={edited}>
      <h3 className={styles.heading}>
        <button
          type="button"
          className={styles.summaryLine}
          aria-expanded={open}
          aria-controls={`${tool.name}-body`}
          onClick={toggle}
        >
          <Chevron className={styles.icon} strokeWidth={1.8} aria-hidden />
          {/* The tool's name, because it is what the model calls and what
           * `tools/offered` records a version under. */}
          <span className={styles.id}>{tool.name}</span>
          <span className={styles.title}>{tool.title}</span>
          {edited && <span className={styles.state}>Edited</span>}
        </button>
      </h3>

      {open && (
        <div className={styles.body} id={`${tool.name}-body`}>
          <p className={styles.why}>{tool.summary}</p>

          {tool.dependants.map((dependant) => (
            <Depends
              key={dependant.note}
              dependant={dependant}
              suppressed={gone.has(dependant.note)}
            />
          ))}

          {outdated(document) && (
            <Moved
              text={document.text}
              shipped={tool.default}
              note={document.note}
              reset={() => void reset(tool.name)}
            />
          )}

          <ShapeList shape={document.shape} parameters={tool.parameters} />

          <textarea
            className={styles.text}
            aria-label={`${tool.title}: description and parameters`}
            spellCheck={false}
            rows={Math.min(Math.max(draft.split('\n').length + 1, 6), 24)}
            value={draft}
            // On leaving the field, for the reason the paragraph editor gives:
            // every write is a version the next turn could offer.
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commit}
          />

          <footer className={styles.foot}>
            <p className={styles.fills}>
              The description, then a <code className={styles.placeholder}>## name</code> section
              per parameter. One document, one version.
            </p>
            {edited && (
              <button type="button" className={styles.revert} onClick={() => void reset(tool.name)}>
                <RotateCcw className={styles.icon} strokeWidth={1.8} aria-hidden />
                Restore the default
              </button>
            )}
          </footer>
        </div>
      )}
    </article>
  )
}

/**
 * The shape, drawn and never edited.
 *
 * What the parser holds a call to: each parameter's name, its type, and
 * whether it is required. In the declaration's order, which is the order the
 * document's sections follow, so the two read against each other line by line.
 * A name the shape does not carry falls back to the declaration alone rather
 * than vanishing, because a document can still give it prose.
 */
function ShapeList({ shape, parameters }: { shape: Shape | null; parameters: string[] }) {
  const required = new Set(shape?.required ?? [])

  return (
    <dl className={styles.shape} aria-label="Shape, fixed">
      <dt className={styles.shapeHead}>
        <Lock className={styles.icon} strokeWidth={1.8} aria-hidden />
        Shape, fixed by the parser
      </dt>
      {parameters.map((name) => {
        const type = shape?.properties?.[name]?.type
        return (
          <dd key={name} className={styles.param}>
            <code className={styles.paramName}>{name}</code>
            <span className={styles.paramType}>
              {Array.isArray(type) ? type.join(' or ') : (type ?? 'any')}
              {required.has(name) ? ', required' : ', optional'}
            </span>
          </dd>
        )
      })}
    </dl>
  )
}
