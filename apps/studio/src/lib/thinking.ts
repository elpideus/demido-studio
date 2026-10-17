import type { EffortLevel, ModelEntry, ModelSettings } from './types';

/** How hard a model thinks, as the composer and the model editor offer it. */
export type ThinkingChoice = 'default' | 'on' | 'off' | EffortLevel;

interface ChoiceInfo {
  label: string;
  description: string;
}

const CHOICES: Record<ThinkingChoice, ChoiceInfo> = {
  default: { label: 'Default', description: 'The level the provider sets for this model.' },
  on: { label: 'On', description: 'Thinks before answering, as long as it needs.' },
  off: { label: 'Off', description: 'Answers directly, without thinking first. Fastest.' },
  minimal: { label: 'Minimal', description: 'Barely thinks. Quickest of the levels.' },
  low: { label: 'Low', description: 'Thinks briefly. Fast and cheap, for simple questions.' },
  medium: { label: 'Medium', description: 'Balances speed and depth.' },
  high: { label: 'High', description: 'Thinks things through. Good for most hard work.' },
  xhigh: { label: 'Extra high', description: 'Thinks longer, for coding and long tasks with tools.' },
  max: { label: 'Max', description: 'Thinks as long as it takes. Slowest, for the hardest problems.' },
};

/** What a choice is called and does; for `model`, the default names the level it stands for. */
export function thinkingInfo(choice: ThinkingChoice, model?: ModelEntry): ChoiceInfo {
  const level = model?.defaultEffort;
  if (choice === 'default' && level) {
    return { label: 'Default', description: `The model’s own level: ${CHOICES[level].label}.` };
  }
  return CHOICES[choice];
}

/** Can be asked how hard to think: it thinks at all. */
export function canThink(model: ModelEntry): boolean {
  return model.capabilities.thinking === true;
}

/** What a model offers, in order: its default and levels least first, or on and off. Off only
 *  where thinking really stops: Claude's and Gemini's newest models always think. */
export function thinkingChoices(model: ModelEntry): ThinkingChoice[] {
  const off: ThinkingChoice[] = model.alwaysThinks ? [] : ['off'];
  if (model.efforts.length === 0) return ['on', ...off];
  return ['default', ...model.efforts, ...off];
}

/** What it is set to now. */
export function currentThinking(model: ModelEntry, settings: ModelSettings = model.settings): ThinkingChoice {
  const choices = thinkingChoices(model);
  const thinking = settings.thinking ?? true;
  const effort = settings.effort ?? null;
  if (!thinking) {
    if (choices.includes('off')) return 'off';
    // Answering directly, on a model that always thinks, is its lowest level.
    if (model.efforts[0]) return model.efforts[0];
  }
  if (model.efforts.length === 0) return 'on';
  return effort && model.efforts.includes(effort) ? effort : 'default';
}

/** The model's settings with `choice` made. */
export function withThinking(settings: ModelSettings, choice: ThinkingChoice): ModelSettings {
  switch (choice) {
    case 'default':
    case 'on':
      return { ...settings, thinking: null, effort: null };
    case 'off':
      return { ...settings, thinking: false, effort: null };
    default:
      return { ...settings, thinking: null, effort: choice };
  }
}
