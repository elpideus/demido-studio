/* Text put into a session's message box from elsewhere in the window (the browser's marks): the box takes it,
   adds it to what is being written, and lets it go. Nothing is sent by it. */
import { create } from 'zustand'

export const useInsert = create<{ waiting: Record<string, string>; put: (session: string, text: string) => void; taken: (session: string) => void }>((set) => ({
  waiting: {},
  put: (session, text) => set((s) => ({ waiting: { ...s.waiting, [session]: s.waiting[session] ? `${s.waiting[session]}\n\n${text}` : text } })),
  taken: (session) => set((s) => { const waiting = { ...s.waiting }; delete waiting[session]; return { waiting } }),
}))
