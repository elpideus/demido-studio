import { useEffect, useState } from 'react'
import {
  ChevronDown,
  ChevronRight,
  CircleSlash,
  Info,
  RotateCcw,
  TriangleAlert,
} from 'lucide-react'

import { useDraft } from './Control'
import { compare } from './diff'
import { outdated, usePrompts, type Dependant, type Prompt } from './register'
import styles from './Prompts.module.css'

/**
 * The prompt editor: the paragraph register, by id, each opening into its text.
 *
 * The brief's
 *
 * > All prompts should be editable.
 *
 * as something a person can open. `docs/rules/prompts.md` decides what that
 * means and this page is the whole of it for the paragraph register; the tool
 * register's editor is S3.
 *
 * **Nothing here is read-only, and no edit is refused for what depends on it.**
 * That was drawn and rejected in writing: it contradicts the brief on the four
 * strings most worth editing, and the honest version of the concern is not
 * forbid but say what breaks. So an entry that is load-bearing renders its
 * dependants above the field, before anything is typed, and an edit
 * **suppresses** the measured claim rather than being turned down. A person may
 * degrade their own classifier; `Origin::Edited` records that they did, and
 * nothing detects that it hurt. That cost is written down rather than glossed.
 *
 * The one thing Rust refuses is a placeholder nothing would ever fill, which is
 * not a judgement about the wording: it is a paragraph that would reach a model
 * as a literal pair of braces.
 *
 * A list that opens rather than a list beside an editor pane. Every entry is a
 * paragraph, so the thing being edited wants the window's whole width, and a
 * second column would spend a third of a page that is already at
 * `--measure-prose` on twelve short ids.
 */
export function PromptsPage() {
  const entries = usePrompts((prompts) => prompts.entries)
  const read = usePrompts((prompts) => prompts.read)
  const [open, setOpen] = useState<string | null>(null)

  useEffect(() => {
    void read()
  }, [read])

  // Nothing rather than an empty page while the first read is in flight. The
  // register is an IPC call away, and a page that drew "no prompts" for a frame
  // would be a page that reports a broken app.
  if (!entries) return <div className={styles.rows} />

  return (
    <div className={styles.rows}>
      {entries.map((prompt) => (
        <Entry
          key={prompt.paragraph.id}
          prompt={prompt}
          open={open === prompt.paragraph.id}
          toggle={() => setOpen(open === prompt.paragraph.id ? null : prompt.paragraph.id)}
        />
      ))}
    </div>
  )
}

/**
 * One entry: its id, and what it says.
 *
 * Closed it is the id and the title, because the id is what the session log
 * names a version by and what the file on disk is called, so it is the thing to
 * find an entry by. Open it is everything true about that entry, in the order a
 * person needs it: what it is for, what depends on it, what this build has since
 * changed, and only then the field.
 *
 * An edited entry marks its row `signal`, which is the accent `design/system.md`
 * already gives a setting row that is saying something of its own rather than
 * following.
 */
function Entry({ prompt, open, toggle }: { prompt: Prompt; open: boolean; toggle: () => void }) {
  const { paragraph } = prompt
  const set = usePrompts((prompts) => prompts.set)
  const reset = usePrompts((prompts) => prompts.reset)
  const edited = prompt.origin === 'edited'
  const [draft, setDraft, commit] = useDraft(prompt.text, async (text) =>
    text === prompt.text ? true : set(paragraph.id, text),
  )

  // The claims this text no longer supports, by the sentence they are rendered
  // as. Rust decides which of them are suppressed, because that follows from
  // the origin and the kind of promise, and the window only has to draw it.
  const gone = new Set(prompt.suppressed.map((dependant) => dependant.note))
  const Chevron = open ? ChevronDown : ChevronRight

  return (
    <article className={styles.entry} data-edited={edited}>
      <h3 className={styles.heading}>
        <button
          type="button"
          className={styles.summaryLine}
          aria-expanded={open}
          aria-controls={`${paragraph.id}-body`}
          onClick={toggle}
        >
          <Chevron className={styles.icon} strokeWidth={1.8} aria-hidden />
          <span className={styles.id}>{paragraph.id}</span>
          <span className={styles.title}>{paragraph.title}</span>
          {/* Said in a word rather than by the accent alone, because the accent
           * is a colour and colour is never the only signal. */}
          {edited && <span className={styles.state}>Edited</span>}
        </button>
      </h3>

      {open && (
        <div className={styles.body} id={`${paragraph.id}-body`}>
          <p className={styles.why}>{paragraph.summary}</p>

          {/* Above the field, which is the whole design: a person knows what
           * they are degrading before they degrade it. */}
          {paragraph.dependants.map((dependant) => (
            <Depends
              key={dependant.note}
              dependant={dependant}
              suppressed={gone.has(dependant.note)}
            />
          ))}

          {outdated(prompt) && <Moved prompt={prompt} reset={() => void reset(paragraph.id)} />}

          <textarea
            className={styles.text}
            aria-label={paragraph.title}
            spellCheck={false}
            rows={Math.min(Math.max(draft.split('\n').length + 1, 6), 24)}
            value={draft}
            // Committed when the field is left rather than on every keystroke.
            // A paragraph is written a sentence at a time, and a write per
            // character is a prompt file rewritten a hundred times per
            // paragraph, each of them a version the next turn could send.
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commit}
          />

          <footer className={styles.foot}>
            <p className={styles.fills}>
              {paragraph.placeholders.length > 0 ? (
                <>
                  Filled in per turn:{' '}
                  {paragraph.placeholders.map((name) => (
                    <code key={name} className={styles.placeholder}>{`{{${name}}}`}</code>
                  ))}
                </>
              ) : (
                'Nothing is filled in: this paragraph is sent as it is written.'
              )}
            </p>
            {/* Only where there is something to go back to. An unedited entry
             * already is what this build ships, and a disabled button would be
             * a control that lies about having something to do. */}
            {edited && (
              <button
                type="button"
                className={styles.revert}
                onClick={() => void reset(paragraph.id)}
              >
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
 * One thing that depends on this exact wording, and whether it still stands.
 *
 * Two kinds and they cost different things. A **measured** claim is a number
 * this repo publishes, and an edit suppresses it: the row is struck out, because
 * what changed is not the wording of the warning but whether the claim is still
 * being made. A **shared** wording is used in more than one place, so an edit
 * changes all of them at once and there is nothing to suppress; it reads the
 * same before and after.
 *
 * The sentence is a dependant's identity here, both as the key and as what a
 * suppressed one is matched by, and the catalog's own
 * `an_entrys_dependants_do_not_repeat_a_sentence` is what makes that safe: two
 * of one entry's dependants saying the same thing would collapse into one row,
 * and the person would be told one of the two things an edit costs them.
 */
function Depends({ dependant, suppressed }: { dependant: Dependant; suppressed: boolean }) {
  const Glyph = suppressed ? CircleSlash : dependant.kind === 'shared' ? Info : TriangleAlert

  return (
    <p className={styles.depends} data-suppressed={suppressed}>
      <Glyph className={styles.icon} strokeWidth={1.8} aria-hidden />
      <span className={styles.note}>{dependant.note}</span>
      {suppressed && <span className={styles.state}>No longer claimed</span>}
    </p>
  )
}

/**
 * The built-in wording moved after this edit was made.
 *
 * The note is Rust's sentence, not one written here: it is the channel
 * `AGENTS.md` rule 6 requires and the register already writes it. What this adds
 * is the half a sentence cannot carry, which is what the two wordings actually
 * differ by, and the one gesture that acts on it.
 *
 * Whether to draw any of this is decided by the two hashes and not by the note,
 * so the note is rendered only if there is one rather than assumed: the section
 * is about a wording that moved, and the diff is the part of it that is true
 * whether or not a sentence came with it.
 *
 * It stays a note. It blocks nothing, the text in force is still the one the
 * person chose, and the next turn sends it either way.
 */
function Moved({ prompt, reset }: { prompt: Prompt; reset: () => void }) {
  const lines = compare(prompt.text, prompt.paragraph.default)

  return (
    <section className={styles.moved}>
      {prompt.note && (
        <p className={styles.depends}>
          <TriangleAlert className={styles.icon} strokeWidth={1.8} aria-hidden />
          <span className={styles.note}>{prompt.note}</span>
        </p>
      )}
      <pre className={styles.diff}>
        {lines.map((line, at) => (
          // Position is the only identity a line of a diff has: two identical
          // lines are two rows, and neither is the other.
          <span key={at} className={styles.line} data-change={line.change}>
            {line.text || ' '}
          </span>
        ))}
      </pre>
      <button type="button" className={styles.revert} onClick={reset}>
        <RotateCcw className={styles.icon} strokeWidth={1.8} aria-hidden />
        Take the new wording
      </button>
    </section>
  )
}
