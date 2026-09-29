import { RefreshCw } from 'lucide-react';
import { Button, Dialog } from '@demido/ui';

import { useUpdates } from '@/stores/updates';

/** Asks before an update restarts the app while a reply is still being written. */
export function RestartDialog() {
  const open = useUpdates((s) => s.confirmingRestart);
  const confirm = useUpdates((s) => s.confirmRestart);
  const cancel = useUpdates((s) => s.cancelRestart);
  return (
    <Dialog
      open={open}
      onClose={cancel}
      title="Restart to update?"
      footer={
        <>
          <Button variant="ghost" onClick={cancel}>
            Cancel
          </Button>
          <Button variant="primary" icon={RefreshCw} onClick={confirm}>
            Restart now
          </Button>
        </>
      }
    >
      A reply is still being written. Restart now anyway?
    </Dialog>
  );
}
