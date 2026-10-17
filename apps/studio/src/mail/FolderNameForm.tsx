import { useId, useState } from 'react';
import { Folder } from 'lucide-react';
import { Button, Field, Notice, TextField } from '@demido/ui';

import { errorText } from '@/lib/api';
import styles from './MailWindow.module.css';

/** Asks for a folder's name, to make it or rename it; `onSubmit` failing shows why. */
export function FolderNameForm({
  label,
  description,
  initial = '',
  submitLabel,
  onSubmit,
  onCancel,
}: {
  label: string;
  description?: string;
  initial?: string;
  submitLabel: string;
  onSubmit: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const id = useId();
  const [name, setName] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    const trimmed = name.trim();
    if (busy || !trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(trimmed);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  return (
    <div className={styles.connect}>
      <Field label={label} htmlFor={`${id}-name`} description={description}>
        <TextField
          id={`${id}-name`}
          icon={Folder}
          value={name}
          invalid={!!error}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && void submit()}
          onFocus={(e) => e.target.select()}
          autoFocus
        />
      </Field>
      {error && <Notice tone="danger">{error}</Notice>}
      <div className={styles.connectActions}>
        <Button variant="ghost" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button variant="primary" loading={busy} disabled={!name.trim()} onClick={() => void submit()}>
          {submitLabel}
        </Button>
      </div>
    </div>
  );
}
