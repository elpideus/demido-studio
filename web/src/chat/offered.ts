import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'

/**
 * The offered set, as the tool picker reasons about it.
 *
 * `docs/rules/tools.md`: **offered** is what the model is shown, the user owns
 * it, and it lives on the settings ladder as `tools.offered`. What is stored is
 * a list of the tool names that are on, or `null`, which is nobody having named
 * a set and so every tool there is. Everything here is a pure function over
 * that value and the groups Rust registered, so the picker holds no copy of the
 * set: it reads the ladder's row and writes a whole new list back.
 *
 * **A write replaces.** An override of the set is the whole set, never a
 * difference from the one below it, because a merging set has no way to say
 * *off*.
 */

/** One row of the picker. The Rust `Offering`. */
export type Group = { group: string; tools: string[] }

/** How a group's switch reads. Some tools on and some off is **partial**, and
 * never drawn as on. */
export type Switched = 'on' | 'partial' | 'off'

/** The tools that are on, given the stored value. */
export function switchedOn(value: unknown, groups: Group[]): Set<string> {
  if (!Array.isArray(value)) return new Set(groups.flatMap((group) => group.tools))
  return new Set(value.filter((name): name is string => typeof name === 'string'))
}

export function groupSwitch(group: Group, on: Set<string>): Switched {
  const count = group.tools.filter((tool) => on.has(tool)).length
  if (count === 0) return 'off'
  return count === group.tools.length ? 'on' : 'partial'
}

/** The list to store after one tool is flipped. */
export function flipTool(value: unknown, groups: Group[], tool: string): string[] {
  const on = switchedOn(value, groups)
  if (on.has(tool)) on.delete(tool)
  else on.add(tool)
  return ordered(on, groups)
}

/** The list to store after a group's switch is pressed. On turns the group off;
 * partial and off both turn all of it on, which is what pressing a switch that
 * is not fully on means everywhere else. */
export function flipGroup(value: unknown, groups: Group[], name: string): string[] {
  const on = switchedOn(value, groups)
  const group = groups.find((each) => each.group === name)
  if (!group) return ordered(on, groups)
  const off = groupSwitch(group, on) === 'on'
  for (const tool of group.tools) {
    if (off) on.delete(tool)
    else on.add(tool)
  }
  return ordered(on, groups)
}

/**
 * The names in registration order, so the same set is always written the same
 * way, and a name no group holds any more is dropped rather than carried
 * forward forever.
 */
function ordered(on: Set<string>, groups: Group[]): string[] {
  return groups.flatMap((group) => group.tools).filter((tool) => on.has(tool))
}

type Tools = {
  /** What Rust registered, or null until it has been asked. */
  groups: Group[] | null
  /** The folder the tools act in, or null when the window has none, which is
   * a model shown no tools whatever the switches say (issue 113). */
  workspace: string | null
  read: () => Promise<void>
}

/** The groups and the folder, read once. They are what this build registered
 * and what the environment named, and neither changes while it runs. */
export const useTools = create<Tools>((set, get) => ({
  groups: null,
  workspace: null,
  read: async () => {
    if (get().groups) return
    try {
      const shelf = await invoke<{ groups: Group[]; workspace: string | null }>('chat_tools')
      set({ groups: shelf.groups, workspace: shelf.workspace })
    } catch (error) {
      console.warn('the tools could not be read', error)
    }
  },
}))
