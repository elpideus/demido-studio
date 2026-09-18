import { useEffect, useState } from 'react'
import { X } from 'lucide-react'

import { AcceleratorControl, ModelFolderControl } from '@/setup/Controls'
import { useDesk } from '@/shell/desk'
import { Field } from './Control'
import { ALWAYS, MODE, OFFERED, useSettings, type Row, type Tier } from './ladder'
import { PromptsPage } from './Prompts'
import { ToolDocumentsPage } from './ToolDocuments'
import styles from './Settings.module.css'

/**
 * The two settings surfaces, and they are one page rendered twice.
 *
 * `design/windows.md`: **the main Settings window holds global values and
 * nothing else.** No scope picker, no layer chip, no resolution to read,
 * because there is only one tier there. A chat gets its own section, and a
 * control in it is in exactly one of two states: following global, or
 * overridden and carrying the way back.
 *
 * That is why the chat's settings are not a page inside the main window. You
 * always know what you are editing because it is the thing you are standing in,
 * and for a conversation that is the conversation: the popover opens from the
 * composer, the way the tool picker does (`design/shell.md`).
 *
 * Neither surface knows how a control is drawn. Both hand a row to [`Field`],
 * which is the component the set-up wizard renders too.
 *
 * The Set-up section is the other half of that rule and the reason
 * `docs/rules/setup.md` section 2 gives for it: the accelerator row here and
 * the accelerator row in the wizard are the same component, so they cannot
 * come to disagree, and nothing the wizard built is thrown away when set-up is
 * over.
 */

/**
 * The settings window, over the desk.
 *
 * A panel frame with a title bar carrying close, and nothing else yet: floating
 * geometry, the maximise gesture, pinning and the draggable seam are the window
 * manager's, and a maximise button that could not snap would be a control that
 * lies about what it does. What is here is the frame those gestures will be
 * added to, at the tokens `design/system.md` gives a panel.
 */
export function Settings() {
  const close = useDesk((desk) => desk.close)
  const [section, setSection] = useState<Section>('conversation')

  // On the document rather than on the section, because the window is opened
  // from the rail and focus is still on the rail's button when it appears. A
  // handler that waited for focus to be inside would be an Escape key that
  // works only after you have clicked something.
  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === 'Escape') close()
    }
    document.addEventListener('keydown', dismiss)
    return () => document.removeEventListener('keydown', dismiss)
  }, [close])

  return (
    <>
      {/* Nothing is dismissed by clicking away: this is a window rather than a
       * menu, and a settings page that vanished because the pointer landed on
       * the transcript would lose a system prompt somebody was halfway through
       * writing. */}
      <div className={styles.scrim} aria-hidden />
      <section className={styles.window} role="dialog" aria-label="Settings">
        <header className={styles.bar}>
          <h1 className={styles.name}>Settings</h1>
          <button type="button" className={styles.close} aria-label="Close" onClick={close}>
            <X className={styles.icon} strokeWidth={1.8} aria-hidden />
          </button>
        </header>
        <div className={styles.body}>
          {/* The nav `design/system.md` gives Settings. It was a column from
           * the first section rather than a heading, because the sections after
           * it are what a page becomes, and a nav that appeared when the second
           * one landed would have moved every row on screen. */}
          <nav className={styles.nav} aria-label="Sections">
            {SECTIONS.map((name) => (
              <button
                key={name}
                type="button"
                className={styles.section}
                aria-current={name === section ? 'page' : undefined}
                onClick={() => setSection(name)}
              >
                {NAMED[name]}
              </button>
            ))}
          </nav>
          <div className={styles.page}>
            {section === 'conversation' && <Page tier="global" />}
            {section === 'prompts' && <PromptsPage />}
            {section === 'tools' && <ToolDocumentsPage />}
            {section === 'setup' && <SetupPage />}
          </div>
        </div>
      </section>
    </>
  )
}

/**
 * The sections the main window has.
 *
 * Prompts is where "All prompts should be editable" lands as a page: the
 * paragraph register, by id, each entry opening into its text
 * (`docs/rules/prompts.md`). Tool prompts is the other register: one document
 * per host tool, its description and its parameter prose
 * ([#77](https://github.com/elpideus/demido-studio/issues/77)). A section of
 * its own rather than a group under Prompts, because the two are keyed
 * differently and a tool document carries a shape a paragraph never has. Set-up
 * is where set-up is changed after the wizard is gone.
 */
const SECTIONS = ['conversation', 'prompts', 'tools', 'setup'] as const

type Section = (typeof SECTIONS)[number]

/** What each section is called on the nav. Here rather than in a ternary at the
 * button, because a third section is what made the ternary a lie. */
const NAMED: Record<Section, string> = {
  conversation: 'Conversation',
  prompts: 'Prompts',
  tools: 'Tool prompts',
  setup: 'Set-up',
}

/**
 * The set-up section: the same controls the wizard drew.
 *
 * No second implementation of either of them, which is the whole point. What
 * differs is the heading over each, because a settings page is read by
 * somebody who already has an install and a wizard is read by somebody who
 * does not.
 */
function SetupPage() {
  return (
    <div className={styles.rows}>
      <section className={styles.group}>
        <h2 className={styles.groupName}>Accelerator</h2>
        <AcceleratorControl />
      </section>
      <section className={styles.group}>
        <h2 className={styles.groupName}>Model folders</h2>
        <ModelFolderControl />
      </section>
    </div>
  )
}

/**
 * This conversation's own settings, from the composer.
 *
 * The chat is the last word
 * (`docs/decisions/0007-a-chat-outranks-its-character.md`), so every row here
 * either follows the global value or overrides it, and the override reaches
 * this conversation and no other.
 */
export function ChatSettings({ close }: { close: () => void }) {
  return (
    <div
      className={styles.popover}
      role="dialog"
      aria-label="This chat"
      onKeyDown={(event) => {
        if (event.key === 'Escape') close()
      }}
    >
      <header className={styles.head}>
        <h2 className={styles.name}>This chat</h2>
        <p className={styles.why}>Overrides the global settings, for this conversation alone.</p>
      </header>
      <Page tier="chat" />
    </div>
  )
}

/**
 * Whether a row belongs on this surface.
 *
 * The offered set is drawn by the tool picker and nowhere else: the popover is
 * its whole surface (`docs/rules/tools.md`). The mode is a chat's to change
 * from the control beside the picker, so the chat's own popover leaves it out
 * rather than drawing a second control for it, and the main window keeps it as
 * the default every new chat opens with.
 *
 * The model folders are one answer per profile (#72), drawn by the Set-up
 * page's folder control, which moves the scan folders with the download folder
 * so that nothing downloaded goes unread.
 *
 * *Always for this tool* is written by the approval row in the transcript and
 * drawn by nothing (#55). It is a record of answers somebody gave about calls
 * they were shown, and a settings page offering to edit it would be a page
 * offering to grant a permission nobody was asked for. Taking one back is the
 * work the Lessons page's shape will do, not a set field.
 */
function drawnOn(tier: Tier, row: Row): boolean {
  // Where a profile keeps its models is the Set-up page's folder control, and
  // no chat can override it.
  if (row.setting.profile) return false
  if (row.setting.id === OFFERED || row.setting.id === ALWAYS) return false
  if (row.setting.id === MODE) return tier === 'global'
  return true
}

/**
 * One tier's rows.
 *
 * The rows are read when the surface opens and read back after every change,
 * because they are a projection of the ladder rather than a copy of it: a
 * global value a chat is not overriding moves when the global tier moves, and a
 * page holding its own copy would show the old one.
 */
function Page({ tier }: { tier: Tier }) {
  const rows = useSettings((settings) => settings.rows[tier])
  const read = useSettings((settings) => settings.read)
  const set = useSettings((settings) => settings.set)
  const clear = useSettings((settings) => settings.clear)

  useEffect(() => {
    void read(tier)
  }, [read, tier])

  // Nothing rather than an empty page while the first read is in flight. A
  // ladder is an IPC call away, and a page that drew "no settings" for a frame
  // would be a page that reports a broken app.
  if (!rows) return <div className={styles.rows} />

  return (
    <div className={styles.rows}>
      {rows
        .filter((row) => drawnOn(tier, row))
        .map((row: Row) => (
          <Field
            key={row.setting.id}
            row={row}
            onChange={(value) => set(tier, row.setting, value)}
            // The global tier has nothing under it to fall back to, so the main
            // window offers no revert. Reverting there would mean restoring a
            // schema default, which is a different gesture and would be wearing
            // the same icon.
            revert={tier === 'global' ? undefined : () => void clear(tier, row.setting)}
          />
        ))}
    </div>
  )
}
