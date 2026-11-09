// What "Search settings" finds: every setting by the words on its row and the words people use for
// it, plus each model and skill by name. Pages mark a setting's row with `data-setting="<focus>"`,
// which the window scrolls to and points at when a result is chosen.

import type { ModelEntry, Skill } from '@/lib/types';
import { PAGES, type SettingsTab } from './pages';

/** A kind of provider the Providers page can start connecting. */
export type ConnectKind = 'gemini' | 'openrouter' | 'anthropic' | 'claudecode';

/** Where a result leads. */
export interface SettingTarget {
  tab: SettingsTab;
  /** The `data-setting` of the row to scroll to and point at. */
  focus?: string;
  /** The model whose editor opens first. */
  model?: string;
  /** The skill that opens. */
  skill?: string;
  /** The provider the Providers page starts connecting. */
  connect?: ConnectKind;
}

export interface SettingEntry {
  id: string;
  kind: 'setting' | 'model' | 'skill';
  label: string;
  /** Where it is on its page, shown after the page's name. */
  where?: string;
  /** Other words that find it. */
  keywords?: string;
  target: SettingTarget;
}

type Static = Omit<SettingEntry, 'kind' | 'id' | 'target'> & { target: SettingTarget };

/** Settings each model has, found in the editor of the model new chats use. */
interface PerModel {
  label: string;
  keywords?: string;
  focus: string;
  where: string;
  /** Only local models have it. */
  local?: boolean;
}

const STATIC: Static[] = [
  // Providers
  {
    label: 'Add a provider',
    keywords: 'cloud connect service account api key',
    target: { tab: 'providers', focus: 'add-provider' },
  },
  {
    label: 'Connect Google Gemini',
    keywords: 'google ai studio api key free tier',
    target: { tab: 'providers', focus: 'add-provider', connect: 'gemini' },
  },
  {
    label: 'Connect OpenRouter',
    keywords: 'free models api key many labs',
    target: { tab: 'providers', focus: 'add-provider', connect: 'openrouter' },
  },
  {
    label: 'Connect Anthropic with an API key',
    keywords: 'claude console api key billed per token',
    target: { tab: 'providers', focus: 'add-provider', connect: 'anthropic' },
  },
  {
    label: 'Connect a Claude subscription',
    keywords: 'anthropic claude code pro max plan sign in account',
    target: { tab: 'providers', focus: 'add-provider', connect: 'claudecode' },
  },
  {
    label: 'API keys and usage',
    keywords: 'replace key quota limits requests left credit refresh model list rename remove provider',
    target: { tab: 'providers' },
  },

  // Models
  {
    label: 'Get models',
    keywords: 'download catalog hugging face gguf find new',
    target: { tab: 'models', focus: 'get-models' },
  },
  {
    label: 'Turn models on or off',
    keywords: 'enable disable activate deactivate show hide model list default use for new chats',
    target: { tab: 'models', focus: 'models-search' },
  },
  {
    label: 'Model folders',
    keywords: 'gguf lm studio scan add folder directory path',
    target: { tab: 'models', focus: 'model-folders' },
  },

  // Skills
  {
    label: 'New skill',
    keywords: 'create write procedure instructions',
    target: { tab: 'skills', focus: 'new-skill' },
  },
  {
    label: 'Skills folder',
    keywords: 'skill.md commands.json slash commands files open folder',
    target: { tab: 'skills', focus: 'skills-folder' },
  },

  // Chats
  {
    label: 'Model for new chats',
    where: 'New chats',
    keywords: 'default model start',
    target: { tab: 'chats', focus: 'default-model' },
  },
  {
    label: 'Compact long chats automatically',
    where: 'Long chats',
    keywords: 'auto compact autocompact summarize summary context window',
    target: { tab: 'chats', focus: 'auto-compact' },
  },
  {
    label: 'Compact at',
    where: 'Long chats',
    keywords: 'tokens threshold autocompact context window limit',
    target: { tab: 'chats', focus: 'compact-at' },
  },

  // Permissions
  {
    label: 'Ask before using tools',
    where: 'Approvals',
    keywords: 'permission mode cautious balanced autonomous approve approval confirm',
    target: { tab: 'permissions', focus: 'permission-mode' },
  },
  {
    label: 'Always allowed tools',
    where: 'Approvals',
    keywords: 'always allow ask again revoke run python commands',
    target: { tab: 'permissions', focus: 'allowed-tools' },
  },
  {
    label: 'Ask before long downloads',
    where: 'Downloads',
    keywords: 'market data download estimate approval minutes seconds',
    target: { tab: 'permissions', focus: 'download-approval' },
  },

  // Voice
  {
    label: 'Microphone',
    keywords: 'mic input device record audio talk',
    target: { tab: 'voice', focus: 'microphone' },
  },
  {
    label: 'Speech model',
    keywords: 'transcription transcribe dictation speech to text asr qwen3-asr whisper',
    target: { tab: 'voice', focus: 'speech-model' },
  },
  {
    label: 'Send my voice to models that can hear',
    keywords: 'audio recording voice message',
    target: { tab: 'voice', focus: 'send-voice' },
  },

  // Mail
  {
    label: 'Load images automatically',
    where: 'Reading',
    keywords: 'email remote images pictures privacy tracking',
    target: { tab: 'mail', focus: 'mail-images' },
  },
  {
    label: 'Mark as read when opened',
    where: 'Reading',
    keywords: 'email unread',
    target: { tab: 'mail', focus: 'mail-mark-read' },
  },
  {
    label: 'Ask before opening links',
    where: 'Reading',
    keywords: 'email links browser phishing confirm',
    target: { tab: 'mail', focus: 'mail-links' },
  },
  {
    label: 'Mail accounts',
    keywords: 'email imap smtp add account password remove',
    target: { tab: 'mail', focus: 'mail-accounts' },
  },

  // Web & browser
  {
    label: 'SearXNG server',
    where: 'Searching',
    keywords: 'web search engine searx self-hosted metasearch',
    target: { tab: 'web', focus: 'searxng' },
  },
  {
    label: 'Ask Exa when a site refuses',
    where: 'Searching',
    keywords: 'web search fetch exa fallback blocked privacy',
    target: { tab: 'web', focus: 'web-exa' },
  },
  {
    label: 'Address bar searches with',
    where: 'Browser',
    keywords: 'search engine duckduckgo google bing brave default browser',
    target: { tab: 'web', focus: 'browser-engine' },
  },
  {
    label: 'Clear browsing data',
    where: 'Browser',
    keywords: 'cookies sign in logins cache history forget privacy',
    target: { tab: 'web', focus: 'browser-data' },
  },

  // AI runtime
  {
    label: 'Loaded model',
    keywords: 'llama.cpp llama-server runtime unload memory status backend cuda vulkan rocm metal cpu',
    target: { tab: 'runtime', focus: 'runtime-status' },
  },
  {
    label: 'Runtime log',
    keywords: 'logs llama-server output errors',
    target: { tab: 'runtime', focus: 'runtime-status' },
  },
  {
    label: 'Set up or change the AI runtime',
    keywords: 'install backend cuda vulkan rocm setup reinstall',
    target: { tab: 'runtime', focus: 'runtime-status' },
  },
  {
    label: 'Components',
    keywords: 'python node.js nodejs uv installed versions',
    target: { tab: 'runtime', focus: 'runtime-components' },
  },
  {
    label: 'Load the default model at startup',
    where: 'Startup',
    keywords: 'preload start faster first answer gpu memory',
    target: { tab: 'runtime', focus: 'preload' },
  },
  {
    label: 'This computer',
    keywords: 'hardware gpu graphics card vram processor cpu memory ram system os',
    target: { tab: 'runtime', focus: 'hardware' },
  },

  // Updates
  {
    label: 'Check for updates',
    keywords: 'version new install upgrade restart download',
    target: { tab: 'updates', focus: 'update-status' },
  },
  {
    label: 'Update channel',
    keywords: 'release pre-release prerelease beta',
    target: { tab: 'updates', focus: 'update-channel' },
  },
  {
    label: 'Update automatically',
    keywords: 'auto update background',
    target: { tab: 'updates', focus: 'auto-update' },
  },

  // About
  {
    label: 'Version',
    keywords: 'demido studio about build',
    target: { tab: 'about', focus: 'about-version' },
  },
  {
    label: 'Your data folder',
    where: 'Storage',
    keywords: 'storage location path chats database files',
    target: { tab: 'about', focus: 'data-folder' },
  },
  {
    label: 'Installation folder',
    where: 'Storage',
    keywords: 'storage location path install',
    target: { tab: 'about', focus: 'install-folder' },
  },
  {
    label: 'Safety notice',
    keywords: 'disclaimer warranty prompt injection risk',
    target: { tab: 'about', focus: 'safety-notice' },
  },
  {
    label: 'License',
    keywords: 'gnu gpl free software open source',
    target: { tab: 'about', focus: 'license' },
  },
];

const PER_MODEL: PerModel[] = [
  {
    label: 'System prompt',
    keywords: 'instructions role tone rules',
    focus: 'model-system-prompt',
    where: 'Instructions',
  },
  { label: 'Model name and picture', keywords: 'rename avatar identity', focus: 'model-name', where: 'Identity' },
  { label: 'Temperature', keywords: 'creative random sampling', focus: 'model-temperature', where: 'Sampling' },
  { label: 'Top P', keywords: 'nucleus sampling', focus: 'model-topP', where: 'Sampling' },
  { label: 'Top K', keywords: 'sampling candidates', focus: 'model-topK', where: 'Sampling' },
  { label: 'Min P', keywords: 'sampling', focus: 'model-minP', where: 'Sampling', local: true },
  {
    label: 'Repeat penalty',
    keywords: 'repetition sampling',
    focus: 'model-repeatPenalty',
    where: 'Sampling',
    local: true,
  },
  { label: 'Thinking', keywords: 'reasoning effort think', focus: 'model-thinking', where: 'Sampling' },
  { label: 'Longest answer', keywords: 'max tokens output length limit', focus: 'model-max-tokens', where: 'Length' },
  {
    label: 'Context length',
    keywords: 'context window tokens memory',
    focus: 'model-context',
    where: 'Length',
    local: true,
  },
  {
    label: 'Compact at, for one model',
    keywords: 'auto compact tokens threshold',
    focus: 'model-compact-at',
    where: 'Length',
  },
  { label: 'GPU layers', keywords: 'offload vram graphics', focus: 'model-gpu-layers', where: 'Length', local: true },
];

/** The model a per-model setting is shown on: the one new chats use, or the first that has it. */
function modelFor(models: ModelEntry[], local: boolean): ModelEntry | undefined {
  const fits = (m: ModelEntry) => !local || m.source === 'local';
  return (
    models.find((m) => m.isDefault && m.enabled && fits(m)) ??
    models.find((m) => m.enabled && fits(m)) ??
    models.find(fits)
  );
}

/** Everything the search can find, with the models and skills there are now. */
export function settingEntries(models: ModelEntry[], skills: Skill[]): SettingEntry[] {
  const entries: SettingEntry[] = STATIC.map((e, i) => ({ ...e, id: `s${i}`, kind: 'setting' }));
  for (const p of PER_MODEL) {
    const model = modelFor(models, !!p.local);
    entries.push({
      id: `m:${p.focus}`,
      kind: 'setting',
      label: p.label,
      where: model ? `${model.name} · ${p.where}` : 'Each model',
      keywords: `${p.keywords ?? ''} model`,
      target: model ? { tab: 'models', model: model.id, focus: p.focus } : { tab: 'models' },
    });
  }
  for (const m of models) {
    entries.push({
      id: `model:${m.id}`,
      kind: 'model',
      label: m.name,
      where: m.source === 'local' ? 'Local model' : (m.providerName ?? 'Cloud model'),
      keywords: `${m.defaultName} ${m.parameters ?? ''} ${m.quant ?? ''}`,
      target: { tab: 'models', model: m.id },
    });
  }
  for (const sk of skills) {
    entries.push({
      id: `skill:${sk.id}`,
      kind: 'skill',
      label: sk.name,
      keywords: sk.description,
      target: { tab: 'skills', skill: sk.id },
    });
  }
  return entries;
}

/** Lowercase, without accents, so "resume" finds "résumé". */
function fold(text: string): string {
  return text
    .normalize('NFD')
    .replace(/\p{Diacritic}/gu, '')
    .toLowerCase();
}

function pageLabel(tab: SettingsTab): string {
  return PAGES.find((p) => p.id === tab)?.label ?? '';
}

/**
 * The entries that have every word of the query, best first: a label that is the query, then one
 * that starts with it, then one with a word that starts with it, then the rest. Settings come
 * before models and skills that match as well, and otherwise the rail's order holds.
 */
export function searchSettings(entries: SettingEntry[], query: string, limit = 12): SettingEntry[] {
  const q = fold(query.trim());
  if (!q) return [];
  const words = q.split(/\s+/);
  const scored: Array<{ entry: SettingEntry; score: number; index: number }> = [];
  entries.forEach((entry, index) => {
    const label = fold(entry.label);
    const haystack = `${label} ${fold(entry.where ?? '')} ${fold(entry.keywords ?? '')} ${fold(pageLabel(entry.target.tab))}`;
    if (!words.every((w) => haystack.includes(w))) return;
    let score = 0;
    if (label === q) score = 100;
    else if (label.startsWith(q)) score = 80;
    else if (label.split(/[\s,/·-]+/).some((w) => w.startsWith(words[0]!))) score = 60;
    else if (label.includes(q)) score = 40;
    if (entry.kind === 'setting') score += 5;
    scored.push({ entry, score, index });
  });
  const order = (tab: SettingsTab) => PAGES.findIndex((p) => p.id === tab);
  scored.sort(
    (a, b) => b.score - a.score || order(a.entry.target.tab) - order(b.entry.target.tab) || a.index - b.index,
  );
  return scored.slice(0, limit).map((s) => s.entry);
}

/** The label split where it contains the query's words, for the matches to stand out. */
export function highlight(label: string, query: string): Array<{ text: string; match: boolean }> {
  const folded = fold(label);
  // Folding can change the length (a letter and its accent become one), and then the positions
  // would not line up with the label: show it plain.
  if (folded.length !== label.length) return [{ text: label, match: false }];
  const marked = new Array<boolean>(label.length).fill(false);
  for (const word of fold(query).split(/\s+/).filter(Boolean)) {
    const at = folded.indexOf(word);
    if (at >= 0) marked.fill(true, at, at + word.length);
  }
  const parts: Array<{ text: string; match: boolean }> = [];
  for (let i = 0; i < label.length; i += 1) {
    const last = parts.at(-1);
    if (last && last.match === marked[i]) last.text += label[i];
    else parts.push({ text: label[i]!, match: marked[i]! });
  }
  return parts;
}
