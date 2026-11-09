import {
  Blocks,
  Cloud,
  Cpu,
  Globe,
  Info,
  Mail,
  MessagesSquare,
  Mic,
  RefreshCw,
  Server,
  ShieldCheck,
  type LucideIcon,
} from 'lucide-react';

export type SettingsTab =
  | 'providers'
  | 'models'
  | 'skills'
  | 'chats'
  | 'permissions'
  | 'voice'
  | 'mail'
  | 'web'
  | 'runtime'
  | 'updates'
  | 'about';

export interface SettingsPage {
  id: SettingsTab;
  label: string;
  /** One line under the page's title. */
  description: string;
  icon: LucideIcon;
  /** The icon tile's color, in the rail, the page header and search results. */
  tint: string;
  group: 'AI' | 'Preferences' | 'System';
}

/** The Settings window's pages, in the rail's order. */
export const PAGES: SettingsPage[] = [
  {
    id: 'providers',
    label: 'Providers',
    description: 'Connect cloud AI services. Their models appear next to your local ones.',
    icon: Cloud,
    tint: '#72a9ee',
    group: 'AI',
  },
  {
    id: 'models',
    label: 'Models',
    description: 'Choose which models appear in the chat and tune how they answer.',
    icon: Cpu,
    tint: '#a993f2',
    group: 'AI',
  },
  {
    id: 'skills',
    label: 'Skills',
    description: 'Saved procedures the assistant follows when a request matches.',
    icon: Blocks,
    tint: '#e9a55a',
    group: 'AI',
  },
  {
    id: 'chats',
    label: 'Chats',
    description: 'The model new chats start with, and how long conversations fit the model.',
    icon: MessagesSquare,
    tint: '#5fd08a',
    group: 'Preferences',
  },
  {
    id: 'permissions',
    label: 'Permissions',
    description: 'When the assistant asks before it acts, and what it may do without asking.',
    icon: ShieldCheck,
    tint: '#ef8080',
    group: 'Preferences',
  },
  {
    id: 'voice',
    label: 'Voice',
    description: 'Talk instead of typing: the microphone and the model that writes down what you say.',
    icon: Mic,
    tint: '#e07cc6',
    group: 'Preferences',
  },
  {
    id: 'mail',
    label: 'Mail',
    description: 'How the Mail window shows and opens your email.',
    icon: Mail,
    tint: '#5cc4e0',
    group: 'Preferences',
  },
  {
    id: 'web',
    label: 'Web & browser',
    description: 'How the assistant searches the web and reads its pages, and the built-in browser.',
    icon: Globe,
    tint: '#9ccc65',
    group: 'Preferences',
  },
  {
    id: 'runtime',
    label: 'AI runtime',
    description: 'The engine that runs local models on this computer, and what it runs on.',
    icon: Server,
    tint: '#4fc9ad',
    group: 'System',
  },
  {
    id: 'updates',
    label: 'Updates',
    description: 'Get new versions of Demido Studio as they come out.',
    icon: RefreshCw,
    tint: '#8d9bf5',
    group: 'System',
  },
  {
    id: 'about',
    label: 'About',
    description: 'Version, license, and where Demido Studio keeps your data.',
    icon: Info,
    tint: '#a3a8b0',
    group: 'System',
  },
];

/** Pages from before General was split up, and where their settings went. */
const MOVED: Record<string, SettingsTab> = { general: 'chats' };

/** The page a window's `tab` prop names, or `fallback` when it names none. */
export function pageFor(tab: unknown, fallback: SettingsTab): SettingsPage {
  const id = typeof tab === 'string' ? (MOVED[tab] ?? tab) : fallback;
  return PAGES.find((p) => p.id === id) ?? PAGES.find((p) => p.id === fallback)!;
}
