import { useState } from 'react';
import { ShieldAlert } from 'lucide-react';
import { Button, Checkbox, Dialog } from '@demido/ui';

import { useApp } from '@/stores/app';
import styles from './Disclaimer.module.css';

/** The safety notice shown on first launch (and again when its wording changes). */
export function Disclaimer() {
  const info = useApp((s) => s.info);
  const accept = useApp((s) => s.acceptDisclaimer);
  const [understood, setUnderstood] = useState(false);
  const open = info !== null && !info.disclaimerAccepted;

  return (
    <Dialog
      open={open}
      onClose={() => undefined}
      dismissible={false}
      width={560}
      icon={<ShieldAlert size={22} strokeWidth={1.8} className={styles.icon} />}
      title="Before you start"
      description="Demido Studio is still under development. Please use it with caution."
      footer={
        <Button variant="primary" disabled={!understood} onClick={() => void accept()}>
          Continue
        </Button>
      }
    >
      <ul className={styles.list}>
        <li>
          <strong>AI can be wrong or be manipulated.</strong> Models can make mistakes, misbehave, or be
          tricked by content they read (prompt injection) into doing things you did not ask for, such as
          running code or writing files.
        </li>
        <li>
          <strong>Stay in control.</strong> Review what the assistant does, and only allow it to run code
          you are comfortable with.
        </li>
        <li>
          <strong>Not advice.</strong> Market data and analysis are for information only. Do not rely on the
          assistant for financial, medical, legal or other important decisions.
        </li>
        <li>
          <strong>No warranty.</strong> The software is provided as is. Its author accepts no responsibility
          for any damage, loss or other consequence of its use.
        </li>
      </ul>
      <Checkbox
        className={styles.check}
        checked={understood}
        onChange={setUnderstood}
        label="I understand these risks and want to continue"
      />
    </Dialog>
  );
}
