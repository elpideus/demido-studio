import { useEffect, useState } from 'react'
import { Check, ChevronRight, Minus, RotateCcw } from 'lucide-react'

import {
  flipGroup,
  flipTool,
  groupSwitch,
  switchedOn,
  useTools,
  type Switched,
} from '@/chat/offered'
import { spoken as titled } from '@/settings/Control'
import { OFFERED, useSettings } from '@/settings/ladder'
import styles from './ToolPicker.module.css'

/**
 * The tool picker: what this conversation offers the model.
 *
 * `design/shell.md` and `docs/rules/tools.md`: a popover from the composer,
 * beside the mode control, because the set is changed **about the message you
 * are about to send**. Rows are groups, expandable in place to their tools,
 * everything is on by default, and a group with some of its tools off is drawn
 * **partial** rather than on. Nothing is delegated to a settings page: this is
 * the whole surface.
 *
 * It holds no set of its own. The value is the chat tier's `tools.offered` row,
 * and a switch writes a whole new list through `settings_set` and reads the
 * ladder back, so what is drawn is what the next turn sends. A tool switched off
 * here is not in the payload at all, which `demido-chat/tests/offered.rs`
 * asserts against what the backend receives.
 *
 * One row per installed skill goes under the built-in groups, with the same
 * switch. No skill is installed in this build, so there are none to draw.
 */
export function ToolPicker({ close }: { close: () => void }) {
  const groups = useTools((tools) => tools.groups)
  const workspace = useTools((tools) => tools.workspace)
  const readGroups = useTools((tools) => tools.read)
  const row = useSettings((settings) => settings.rows.chat?.find((it) => it.setting.id === OFFERED))
  const read = useSettings((settings) => settings.read)
  const set = useSettings((settings) => settings.set)
  const clear = useSettings((settings) => settings.clear)
  const [open, setOpen] = useState<string[]>([])

  useEffect(() => {
    void readGroups()
    void read('chat')
  }, [readGroups, read])

  const on = row && groups ? switchedOn(row.value, groups) : null
  // With no folder the backend is handed no tools whatever the ladder says, so
  // every switch is drawn off and held, and so is the chip that would change
  // the set (issue 113). The ladder's value is untouched either way.
  const held = groups !== null && workspace === null

  return (
    <div
      className={styles.popover}
      role="dialog"
      aria-label="Tools"
      onKeyDown={(event) => {
        if (event.key === 'Escape') close()
      }}
    >
      <header className={styles.head}>
        <div className={styles.title}>
          <h2 className={styles.name}>Tools</h2>
          {/* The way back to the set every chat opens with, and only where this
           * chat has named its own. The same chip a settings row carries. */}
          {row?.setHere && !held && (
            <button
              type="button"
              className={styles.revert}
              onClick={() => void clear('chat', row.setting)}
            >
              <RotateCcw className={styles.icon} strokeWidth={1.8} aria-hidden />
              Follow global
            </button>
          )}
        </div>
        {held ? (
          <p className={styles.why}>
            This window has no folder for tools to act in, so the model is shown none of them.
          </p>
        ) : (
          <p className={styles.why}>
            What the model is shown in this chat. A tool switched off is not sent to it at all.
            {workspace && (
              <>
                {' '}
                They act in <span className={styles.folder}>{workspace}</span>.
              </>
            )}
          </p>
        )}
      </header>
      {/* Nothing rather than an empty list while the first read is in flight,
       * for the reason a settings page draws nothing: a list with no rows for a
       * frame is a list that reports a broken app. */}
      {row && groups && on && (
        <ul className={styles.groups}>
          {groups.map((group) => {
            const expanded = open.includes(group.group)
            const switched = groupSwitch(group, on)
            return (
              <li key={group.group} className={styles.group}>
                <div className={styles.row}>
                  <button
                    type="button"
                    className={styles.expand}
                    aria-expanded={expanded}
                    onClick={() =>
                      setOpen(
                        expanded
                          ? open.filter((name) => name !== group.group)
                          : [...open, group.group],
                      )
                    }
                  >
                    <ChevronRight className={styles.chevron} strokeWidth={1.8} aria-hidden />
                    <span className={styles.label}>{titled(group.group)}</span>
                    <span className={styles.count}>
                      {group.tools.length === 1 ? '1 tool' : `${group.tools.length} tools`}
                    </span>
                  </button>
                  <Switch
                    label={titled(group.group)}
                    state={held ? 'off' : switched}
                    held={held}
                    onPress={() =>
                      void set('chat', row.setting, flipGroup(row.value, groups, group.group))
                    }
                  />
                </div>
                {expanded && (
                  <ul className={styles.tools}>
                    {group.tools.map((tool) => (
                      <li key={tool} className={styles.row}>
                        <span className={styles.tool}>{tool}</span>
                        <Switch
                          label={tool}
                          state={!held && on.has(tool) ? 'on' : 'off'}
                          held={held}
                          onPress={() =>
                            void set('chat', row.setting, flipTool(row.value, groups, tool))
                          }
                        />
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

/**
 * A switch with a third state.
 *
 * A checkbox by role, because `mixed` is a checkbox's word and a switch has no
 * way to say partial. The state is carried by the thing itself
 * (`design/shell.md`): lit with a check when on, a bar when partial, empty when
 * off, and the words beside it never change colour to say so.
 */
function Switch({
  label,
  state,
  held = false,
  onPress,
}: {
  label: string
  state: Switched
  /** Drawn but not pressable: there is nothing a press would send. */
  held?: boolean
  onPress: () => void
}) {
  return (
    <button
      type="button"
      role="checkbox"
      className={styles.switch}
      disabled={held}
      aria-label={label}
      aria-checked={state === 'partial' ? 'mixed' : state === 'on'}
      data-state={state}
      onClick={onPress}
    >
      {state === 'on' && <Check className={styles.mark} strokeWidth={1.8} aria-hidden />}
      {state === 'partial' && <Minus className={styles.mark} strokeWidth={1.8} aria-hidden />}
    </button>
  )
}
