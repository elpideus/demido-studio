/* The models a person may pick for work done away from the sessions screen (a schedule, voice mode): those on
   this computer, and those chosen through an API. A program such as Claude Code is not among them: its terms
   are for a person's own use, not for runs with nobody there (U23), and it has not been tried in voice mode. */
import { useLocal } from './local-models'
import { chosenModels, useProviders } from './providers'

export function useModelChoices(): { value: string; label: string }[] {
  const local = useLocal((s) => s.list?.models) ?? []
  const providers = useProviders((s) => s.providers)
  const api = chosenModels(providers?.filter((p) => p.kind === 'api') ?? null)
  return [...local.map((m) => ({ value: m.id, label: m.state === 'loaded' ? `${m.id}, loaded` : m.id })), ...api.map((m) => ({ value: m.id, label: `${m.name}, through ${m.provider}` }))]
}
