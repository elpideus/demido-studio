import { Shield, ShieldCheck, ShieldOff, type LucideIcon } from 'lucide-react';

import type { PermissionMode } from './types';

interface ModeInfo {
  value: PermissionMode;
  label: string;
  icon: LucideIcon;
  description: string;
}

const BALANCED: ModeInfo = {
  value: 'balanced',
  label: 'Balanced',
  icon: Shield,
  description:
    'Asks only before risky steps: running code, sending email, deleting for good, saving to TradingView, long downloads.',
};

/** The permission modes as the composer and Settings offer them, most careful first. */
export const PERMISSION_MODES: ModeInfo[] = [
  {
    value: 'cautious',
    label: 'Cautious',
    icon: ShieldCheck,
    description: 'Asks before every tool that reads or changes anything: files, email, prices, code.',
  },
  BALANCED,
  {
    value: 'autonomous',
    label: 'Autonomous',
    icon: ShieldOff,
    description:
      'Never asks. Emails and web pages the assistant reads could steer it into acting on them: use it with care.',
  },
];

/** The mode `value` names; Balanced, the default, for a value this UI does not know. */
export function permissionMode(value: PermissionMode | undefined): ModeInfo {
  return PERMISSION_MODES.find((m) => m.value === value) ?? BALANCED;
}
