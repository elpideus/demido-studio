import { useMemo, useRef, useState } from 'react';
import { BookMarked, FileText, LineChart, Settings2, Sparkles, Terminal, Wrench, type LucideIcon } from 'lucide-react';
import { Popover, SearchList, Switch, type SearchListItem } from '@demido/ui';

import { useSkills } from '@/stores/skills';
import { useWindows } from '@/stores/windows';
import styles from './Composer.module.css';

const GROUP_ICONS: Record<string, LucideIcon> = {
  market: LineChart,
  python: Terminal,
  files: FileText,
  skills: Sparkles,
};

/** The composer's Tools button: switch tool groups and skills on and off. */
export function ToolsPicker() {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const groups = useSkills((s) => s.groups);
  const skills = useSkills((s) => s.skills);
  const toggleGroup = useSkills((s) => s.toggleGroup);
  const toggleSkill = useSkills((s) => s.toggleSkill);
  const openWindow = useWindows((s) => s.open);

  const active = groups.filter((g) => g.enabled && g.available).length + skills.filter((s) => s.enabled).length;

  const items: SearchListItem[] = useMemo(
    () => [
      ...groups.map((g) => {
        const Icon = GROUP_ICONS[g.id] ?? Wrench;
        return {
          id: `group:${g.id}`,
          label: g.label,
          description: g.available ? g.description : (g.reason ?? 'Unavailable'),
          keywords: g.tools.join(' '),
          group: 'Tools',
          disabled: !g.available,
          leading: <Icon size={16} strokeWidth={1.8} aria-hidden />,
          trailing: (
            <Switch
              size="sm"
              checked={g.enabled && g.available}
              disabled={!g.available}
              onChange={(v) => void toggleGroup(g.id, v)}
              label={g.label}
            />
          ),
        };
      }),
      ...skills.map((s) => ({
        id: `skill:${s.id}`,
        label: s.name,
        description: s.problem ?? s.description,
        keywords: s.id,
        group: 'Skills',
        disabled: !!s.problem,
        leading: <BookMarked size={16} strokeWidth={1.8} aria-hidden />,
        trailing: (
          <Switch size="sm" checked={s.enabled} disabled={!!s.problem} onChange={(v) => void toggleSkill(s.id, v)} label={s.name} />
        ),
      })),
    ],
    [groups, skills, toggleGroup, toggleSkill],
  );

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={styles.chip}
        onClick={() => setOpen(!open)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label="Tools and skills"
      >
        <Wrench size={15} strokeWidth={1.8} aria-hidden />
        <span className={styles.chipLabel}>Tools</span>
        {active > 0 && <span className={styles.count}>{active}</span>}
      </button>
      <Popover open={open} onClose={() => setOpen(false)} anchorRef={anchor} placement="top-start" width={380} aria-label="Tools">
        <SearchList
          items={items}
          placeholder="Search tools and skills"
          emptyText="No tools or skills match."
          onSelect={(item) => {
            if (item.id.startsWith('group:')) {
              const g = groups.find((x) => `group:${x.id}` === item.id);
              if (g && g.available) void toggleGroup(g.id, !g.enabled);
            } else {
              const s = skills.find((x) => `skill:${x.id}` === item.id);
              if (s) void toggleSkill(s.id, !s.enabled);
            }
          }}
          footer={{
            icon: Settings2,
            label: 'Settings',
            description: 'Manage, edit and create skills',
            onSelect: () => {
              setOpen(false);
              openWindow('settings', { tab: 'skills' });
            },
          }}
        />
      </Popover>
    </>
  );
}
