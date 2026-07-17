import { create } from 'zustand'
import { skills as skillsApi } from '../lib/tauri'
import { useSkills } from './skills'
import { getTypeForFile } from '../lib/parseArtifacts'
import type { Artifact, SkillFile } from '../types'

/** An open skill folder. Its files ride the artifact viewer, one tab per file. */
interface SkillSession {
  skillId: string
  skillName: string
  files: SkillFile[]
  activeFile: string
}

function toArtifact(session: SkillSession, file: SkillFile): Artifact {
  return {
    id: `skill:${session.skillId}:${file.name}`,
    messageId: '',
    type: getTypeForFile(file.name),
    title: file.name,
    content: file.content,
  }
}

interface ArtifactsState {
  activeArtifact: Artifact | null
  poppedOut: boolean
  skillSession: SkillSession | null
  setActive(artifact: Artifact | null): void
  setPoppedOut(poppedOut: boolean): void
  /** Load a skill's files and show them in the viewer. Resolves once the first tab is up. */
  openSkill(skillId: string, skillName: string): Promise<void>
  selectSkillFile(name: string): void
  saveSkillFile(name: string, content: string): Promise<void>
}

export const useArtifacts = create<ArtifactsState>((set, get) => ({
  activeArtifact: null,
  poppedOut: false,
  skillSession: null,

  setActive: artifact =>
    set({
      activeArtifact: artifact,
      ...(artifact ? {} : { poppedOut: false, skillSession: null }),
    }),

  setPoppedOut: poppedOut => set({ poppedOut }),

  openSkill: async (skillId, skillName) => {
    const files = await skillsApi.readFiles(skillId)
    if (!files.length) throw new Error('This skill has no editable files.')
    const session: SkillSession = { skillId, skillName, files, activeFile: files[0].name }
    set({ skillSession: session, activeArtifact: toArtifact(session, files[0]) })
  },

  selectSkillFile: name => {
    const session = get().skillSession
    const file = session?.files.find(f => f.name === name)
    if (!session || !file) return
    set({ skillSession: { ...session, activeFile: name }, activeArtifact: toArtifact(session, file) })
  },

  saveSkillFile: async (name, content) => {
    const session = get().skillSession
    if (!session) return
    await skillsApi.writeFile(session.skillId, name, content)
    const files = session.files.map(f => (f.name === name ? { ...f, content } : f))
    set({ skillSession: { ...session, files } })
    // The skills store holds SKILL.md's text and feeds it into the system prompt, so it has
    // to re-read or the model keeps seeing the pre-edit version.
    await useSkills.getState().load()
  },
}))
