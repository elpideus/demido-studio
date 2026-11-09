import { Check } from 'lucide-react';
import { Button, Select, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { PERMISSION_MODES, permissionMode } from '@/lib/permissionModes';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import { Group, PageHeader, Row } from './layout';
import s from './settings.module.css';

const TOOL_LABELS: Record<string, string> = {
  run_python: 'Run Python code',
  run_command: 'Run commands',
  computer_control: 'Use the mouse and keyboard',
  pine_publish: 'Save Pine scripts to TradingView',
  mail_mark: 'Mark emails read or starred',
  mail_move: 'Move and copy emails',
  mail_delete: 'Move emails to the trash',
  mail_folder: 'Create and rename mail folders',
  mail_draft: 'Save email drafts',
};

const APPROVAL_CHOICES: Array<{ value: string; label: string }> = [
  { value: '30', label: '30 s' },
  { value: '60', label: '1 min' },
  { value: '120', label: '2 min' },
  { value: '300', label: '5 min' },
];

/** The fixed choices, plus the saved value when it was set to something else (by hand in settings.json). */
function approvalOptions(current: number): Array<{ value: string; label: string }> {
  if (APPROVAL_CHOICES.some((c) => c.value === String(current))) return APPROVAL_CHOICES;
  return [...APPROVAL_CHOICES, { value: String(current), label: `${current} s` }].sort(
    (a, b) => Number(a.value) - Number(b.value),
  );
}

export function PermissionsTab() {
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  if (!settings) return null;
  const mode = permissionMode(settings.permissionMode).value;
  // A backend from before this setting sends none; 60 s is its default.
  const approvalSeconds = settings.downloadApprovalSeconds ?? 60;

  return (
    <div className={s.page}>
      <PageHeader page="permissions" />
      <div className={s.scroll}>
        <Group
          title="When the assistant asks"
          setting="permission-mode"
          footer="You can also change it from the message box. It applies to every chat."
        >
          <div role="radiogroup" aria-label="When the assistant asks before using a tool" className={s.choices}>
            {PERMISSION_MODES.map(({ value, label, icon: Icon, description }) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={value === mode}
                className={cx(s.row, s.rowButton, s.choice)}
                onClick={() => void patch({ permissionMode: value })}
              >
                <span className={cx(s.rowIcon, value === mode && s.choiceIconOn)}>
                  <Icon size={18} strokeWidth={1.8} aria-hidden />
                </span>
                <span className={s.rowMain}>
                  <span className={s.rowTitle}>{label}</span>
                  <span className={s.rowDescription}>{description}</span>
                </span>
                <span className={s.check} aria-hidden>
                  {value === mode && <Check size={16} strokeWidth={2.4} />}
                </span>
              </button>
            ))}
          </div>
        </Group>

        <Group
          title="Always allowed"
          setting="allowed-tools"
          footer="Choosing “Always allow” on a request in Balanced mode adds the tool here. Cautious mode asks anyway."
        >
          {settings.alwaysAllowedTools.length === 0 ? (
            <Row label="No tool is always allowed" />
          ) : (
            settings.alwaysAllowedTools.map((tool) => (
              <Row key={tool} label={TOOL_LABELS[tool] ?? tool} description="Runs without asking in Balanced mode">
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    void api.revokeToolPermission(tool).then(
                      () => useApp.getState().init(),
                      (e) => toast.error('Could not update', errorText(e)),
                    )
                  }
                >
                  Ask again
                </Button>
              </Row>
            ))
          )}
        </Group>

        <Group title="Downloads">
          <Row
            setting="download-approval"
            label="Ask before downloads longer than"
            description="For a longer market data download, the assistant shows the estimate and waits for your OK, except in Autonomous mode."
          >
            <Select
              size="sm"
              aria-label="Ask before downloads longer than"
              value={String(approvalSeconds)}
              onChange={(e) => void patch({ downloadApprovalSeconds: Number(e.target.value) })}
              options={approvalOptions(approvalSeconds)}
            />
          </Row>
        </Group>
      </div>
    </div>
  );
}
