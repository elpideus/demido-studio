import { useId, useState } from 'react';
import { Tag } from 'lucide-react';
import { Button, Field, Notice, TextField } from '@demido/ui';

import { errorText } from '@/lib/api';
import type { MailAccount } from '@/lib/types';
import { useMail } from '@/stores/mail';
import styles from './MailWindow.module.css';

/** The longest name an account can have (MAX_NICKNAME in the backend). */
export const MAX_NAME = 40;

/** Names an account, so the assistant can be asked about it by that name. */
export function NameForm({
  account,
  onDone,
  onCancel,
}: {
  account: MailAccount;
  onDone: (account: MailAccount) => void;
  onCancel: () => void;
}) {
  const id = useId();
  const [name, setName] = useState(account.nickname ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      onDone(await useMail.getState().rename(account.id, name.trim()));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.connect}>
      <Field
        label="Name"
        htmlFor={`${id}-name`}
        description="A short name like Work or Personal: ask the assistant about this account by its name. Leave it empty for none."
      >
        <TextField
          id={`${id}-name`}
          icon={Tag}
          placeholder="Work"
          value={name}
          maxLength={MAX_NAME}
          invalid={!!error}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void submit()}
          autoFocus
        />
      </Field>
      {error && <Notice tone="danger">{error}</Notice>}
      <div className={styles.connectActions}>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" loading={busy} onClick={() => void submit()}>
          Save
        </Button>
      </div>
    </div>
  );
}
