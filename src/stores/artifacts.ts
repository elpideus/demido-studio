import { create } from 'zustand'
import type { Artifact } from '../types'

interface ArtifactsState {
  activeArtifact: Artifact | null
  setActive(artifact: Artifact | null): void
}

export const useArtifacts = create<ArtifactsState>(set => ({
  activeArtifact: null,
  setActive: artifact => set({ activeArtifact: artifact }),
}))
