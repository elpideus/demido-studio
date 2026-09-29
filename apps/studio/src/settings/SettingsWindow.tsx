import { Blocks, Cloud, Cpu, RefreshCw, SlidersHorizontal } from 'lucide-react';

import { type WindowState, useWindows } from '@/stores/windows';
import { TabbedLayout, type TabSpec } from '@/wm/TabbedLayout';
import { GeneralTab } from './GeneralTab';
import { ModelsTab } from './models/ModelsTab';
import { ProvidersTab } from './ProvidersTab';
import { SkillsTab } from './skills/SkillsTab';
import { UpdatesTab } from './UpdatesTab';

export type SettingsTab = 'providers' | 'models' | 'skills' | 'general' | 'updates';

const TABS: Array<TabSpec<SettingsTab>> = [
  { id: 'providers', label: 'Providers', icon: Cloud },
  { id: 'models', label: 'Models', icon: Cpu },
  { id: 'skills', label: 'Skills', icon: Blocks },
  { id: 'general', label: 'General', icon: SlidersHorizontal },
  { id: 'updates', label: 'Updates', icon: RefreshCw },
];

export function SettingsWindow({ win }: { win: WindowState }) {
  const setProps = useWindows((s) => s.setProps);
  const tab = (TABS.some((t) => t.id === win.props.tab) ? win.props.tab : 'models') as SettingsTab;
  return (
    <TabbedLayout
      tabs={TABS}
      active={tab}
      onChange={(id) => setProps(win.id, { tab: id, view: undefined, skill: undefined })}
    >
      {tab === 'providers' && <ProvidersTab />}
      {tab === 'models' && <ModelsTab win={win} />}
      {tab === 'skills' && <SkillsTab win={win} />}
      {tab === 'general' && <GeneralTab />}
      {tab === 'updates' && <UpdatesTab />}
    </TabbedLayout>
  );
}
