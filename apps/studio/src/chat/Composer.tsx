import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { ArrowUp, Plus, Square } from 'lucide-react';
import { IconButton, Spinner, TextArea, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { Attachment, ModelEntry, SlashCommand } from '@/lib/types';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { AttachmentTray, type StagedFile } from './Attachments';
import { ATTACH_FILES_EVENT, MAX_FILE_BYTES, MAX_FILES, nameFromPath, pastedName } from './attachmentView';
import { ModelPicker } from './ModelPicker';
import { commandIn, completion, matchCommands, runsWhenPicked, typedName } from './slashView';
import { ToolsPicker } from './ToolsPicker';
import styles from './Composer.module.css';

let nextKey = 1;

/**
 * Runs the tasks given to it at most `n` at a time. Pasted files are read into memory to be
 * sent, so ten large ones pasted together would all be held at once; files from disk are read
 * by the backend, which already takes them two at a time.
 */
function atMost(n: number) {
  let running = 0;
  const waiting: Array<() => void> = [];
  return <T,>(task: () => Promise<T>): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      const start = () => {
        running += 1;
        task()
          .then(resolve, reject)
          .finally(() => {
            running -= 1;
            waiting.shift()?.();
          });
      };
      if (running < n) start();
      else waiting.push(start);
    });
}

const pasteSlot = atMost(2);

interface Props {
  chatId: string | null;
  model: ModelEntry | undefined;
  /** Text to put in the composer (from a suggestion), then cleared by the parent. */
  prefill?: string | null;
}

export function Composer({ chatId, model, prefill }: Props) {
  const [text, setText] = useState('');
  const [files, setFiles] = useState<StagedFile[]>([]);
  // Keys of the chips on screen. A file that finishes reading after its chip was removed is
  // discarded instead of coming back.
  const shown = useRef(new Set<string>());
  const ref = useRef<HTMLTextAreaElement>(null);
  const running = useChats((s) => (chatId ? !!s.running[chatId] : false));
  const send = useChats((s) => s.send);
  const runCommand = useChats((s) => s.runCommand);
  const stop = useChats((s) => s.stop);
  const pickModel = useChats((s) => s.pickModel);
  const runtime = useModels((s) => s.runtime);
  const skills = useSkills((s) => s.skills);
  const [commands, setCommands] = useState<SlashCommand[]>([]);
  // The highlighted command in the list, and the text at which Escape closed the list.
  const [picked, setPicked] = useState(0);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const menu = useRef<HTMLDivElement>(null);

  // Skills bring commands along, so the list follows them.
  useEffect(() => {
    api.listSlashCommands().then(setCommands, () => undefined);
  }, [skills]);

  /** Shows a chip for each file right away (as many as fit in a message), then reads them. */
  const attach = useCallback((items: Array<{ name: string; read: () => Promise<Attachment> }>) => {
    const taken = items.slice(0, Math.max(0, MAX_FILES - shown.current.size));
    const left = items.length - taken.length;
    if (left > 0) {
      toast.warning(
        `A message can carry up to ${MAX_FILES} files`,
        `${left} ${left === 1 ? 'file was' : 'files were'} not added.`,
      );
    }
    if (taken.length === 0) return;
    const added = taken.map((item) => ({ key: `staged-${nextKey++}`, name: item.name, attachment: null }));
    for (const f of added) shown.current.add(f.key);
    setFiles((list) => [...list, ...added]);
    taken.forEach((item, i) => {
      const key = added[i]!.key;
      item.read().then(
        (attachment) => {
          if (!shown.current.has(key)) {
            void api.discardAttachment(attachment.id).catch(() => undefined);
            return;
          }
          setFiles((list) => list.map((f) => (f.key === key ? { ...f, name: attachment.name, attachment } : f)));
        },
        (e) => {
          shown.current.delete(key);
          setFiles((list) => list.filter((f) => f.key !== key));
          toast.error(`Could not add ${item.name}`, errorText(e));
        },
      );
    });
  }, []);

  const attachPaths = useCallback(
    (paths: string[]) => attach(paths.map((path) => ({ name: nameFromPath(path), read: () => api.attachFile(path) }))),
    [attach],
  );

  const attachBlobs = (blobs: File[]) => {
    const now = new Date();
    // Checked before the bytes are read: a pasted video would otherwise be copied into memory
    // twice before the backend could refuse it.
    const tooBig = blobs.filter((blob) => blob.size > MAX_FILE_BYTES);
    for (const blob of tooBig) {
      toast.error(
        `Could not add ${blob.name || 'the pasted file'}`,
        'It is larger than 100 MB, the most a file can be.',
      );
    }
    attach(
      blobs
        .filter((blob) => blob.size <= MAX_FILE_BYTES)
        .map((blob) => {
          const name = pastedName(blob.name, blob.type, now);
          return {
            name,
            read: () => pasteSlot(async () => api.attachData(name, new Uint8Array(await blob.arrayBuffer()))),
          };
        }),
    );
  };

  const remove = (key: string) => {
    const file = files.find((f) => f.key === key);
    shown.current.delete(key);
    setFiles((list) => list.filter((f) => f.key !== key));
    if (file?.attachment) void api.discardAttachment(file.attachment.id).catch(() => undefined);
    ref.current?.focus();
  };

  const pickFiles = async () => {
    const picked = await openDialog({ multiple: true, title: 'Add files' });
    if (Array.isArray(picked) && picked.length > 0) attachPaths(picked);
    ref.current?.focus();
  };

  useEffect(() => {
    ref.current?.focus();
  }, [chatId]);

  useEffect(() => {
    if (prefill) {
      setText(prefill);
      ref.current?.focus();
    }
  }, [prefill]);

  useEffect(() => {
    const onFocus = () => ref.current?.focus();
    window.addEventListener('demido:focus-composer', onFocus);
    return () => window.removeEventListener('demido:focus-composer', onFocus);
  }, []);

  useEffect(() => {
    const onAttach = (e: Event) => {
      attachPaths((e as CustomEvent<string[]>).detail);
      ref.current?.focus();
    };
    window.addEventListener(ATTACH_FILES_EVENT, onAttach);
    return () => window.removeEventListener(ATTACH_FILES_EVENT, onAttach);
  }, [attachPaths]);

  const reading = files.some((f) => !f.attachment);
  const canSend = !!model && !running && !reading && (!!text.trim() || files.length > 0);

  const typed = typedName(text);
  const matches = typed === null ? [] : matchCommands(commands, typed);
  const menuOpen = matches.length > 0 && dismissed !== text;
  const active = Math.min(picked, matches.length - 1);
  // While its arguments are typed, what the command takes.
  const using = menuOpen ? null : commandIn(commands, text);

  useEffect(() => setPicked(0), [typed]);

  useLayoutEffect(() => {
    if (menuOpen) menu.current?.children[active]?.scrollIntoView({ block: 'nearest' });
  }, [menuOpen, active]);

  /** Sends `value` (the composer's text unless given), running it when it names a command. */
  const submit = async (value = text.trim()) => {
    if (!model || running || reading || (!value && files.length === 0)) return;
    // The app's own commands act on the chat, and the files stay staged for the next message.
    const command = commandIn(commands, value)?.command;
    const sent = command && !command.skill ? [] : files;
    const ids = sent.flatMap((f) => (f.attachment ? [f.attachment.id] : []));
    setText('');
    setDismissed(null);
    if (sent.length > 0) {
      setFiles((list) => list.filter((f) => !sent.includes(f)));
      for (const f of sent) shown.current.delete(f.key);
    }
    const ok = command ? await runCommand(value, model.id, ids) : await send(value, model.id, ids);
    if (!ok) {
      setText(value);
      if (sent.length > 0) {
        for (const f of sent) shown.current.add(f.key);
        setFiles((list) => [...sent, ...list]);
      }
    }
  };

  /** Picks `command` from the list: runs it when it needs nothing more, or completes its name. */
  const choose = (command: SlashCommand, run: boolean) => {
    if (run && runsWhenPicked(command)) {
      void submit(`/${command.name}`);
    } else {
      setText(completion(command));
      ref.current?.focus();
    }
  };

  /** Moves through the command list and picks from it; true when it took the key. */
  const onMenuKey = (e: KeyboardEvent): boolean => {
    if (!menuOpen || e.nativeEvent.isComposing) return false;
    const command = matches[active]!;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      const step = e.key === 'ArrowDown' ? 1 : -1;
      setPicked((active + step + matches.length) % matches.length);
    } else if (e.key === 'Tab' && !e.shiftKey) {
      choose(command, false);
    } else if (e.key === 'Enter' && !e.shiftKey) {
      choose(command, true);
    } else if (e.key === 'Escape') {
      setDismissed(text);
    } else {
      return false;
    }
    e.preventDefault();
    return true;
  };

  const loading = runtime?.state === 'loading' && model && runtime.modelId === model.id;
  const loadError = runtime?.state === 'error' && model && runtime.modelId === model.id ? runtime.message : null;

  return (
    <div className={styles.wrap}>
      {menuOpen && (
        <div ref={menu} className={styles.commands} role="listbox" aria-label="Commands">
          {matches.map((c, i) => (
            <div
              key={c.name}
              role="option"
              aria-selected={i === active}
              className={cx(styles.command, i === active && styles.commandActive)}
              // Keeps the focus in the composer.
              onMouseDown={(e) => {
                e.preventDefault();
                choose(c, true);
              }}
              onMouseEnter={() => setPicked(i)}
            >
              <span className={styles.commandName}>/{c.name}</span>
              {c.args && <span className={styles.commandArgs}>{c.args}</span>}
              <span className={styles.commandText}>{c.description}</span>
              {c.skillName && <span className={styles.commandSkill}>{c.skillName}</span>}
            </div>
          ))}
        </div>
      )}
      {loading && (
        <div className={styles.notice}>
          <Spinner size={12} /> Loading {model.name} into memory…
        </div>
      )}
      {loadError && <div className={styles.noticeError}>{loadError}</div>}
      {using && (
        <div className={styles.notice}>
          <span className={styles.commandName}>/{using.command.name}</span>
          {using.command.args && <span className={styles.commandArgs}>{using.command.args}</span>}
          <span className={styles.commandText}>{using.command.description}</span>
        </div>
      )}
      <div className={styles.composer}>
        {files.length > 0 && <AttachmentTray files={files} model={model} onRemove={remove} />}
        <TextArea
          ref={ref}
          className={styles.input}
          autoSize={{ min: 1, max: 12 }}
          value={text}
          placeholder={model ? `Message ${model.name}` : 'Choose a model to start'}
          onChange={(e) => {
            setText(e.target.value);
            setDismissed(null);
          }}
          onKeyDown={(e) => {
            if (onMenuKey(e)) return;
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void submit();
            }
          }}
          onPaste={(e) => {
            const pasted = Array.from(e.clipboardData.files);
            // Office copies a picture of the selection along with its text; that paste is text.
            if (pasted.length === 0 || e.clipboardData.getData('text/plain')) return;
            e.preventDefault();
            attachBlobs(pasted);
          }}
          aria-label="Message"
          rows={1}
        />
        <div className={styles.toolbar}>
          <IconButton
            icon={Plus}
            label="Add files"
            size="md"
            strokeWidth={2}
            className={styles.add}
            onClick={() => void pickFiles()}
          />
          <ModelPicker model={model} onPick={pickModel} />
          <ToolsPicker model={model} />
          <span className={styles.spacer} />
          {running ? (
            <IconButton
              icon={Square}
              label="Stop"
              variant="solid"
              size="md"
              className={styles.stop}
              onClick={() => void stop()}
            />
          ) : (
            <IconButton
              icon={ArrowUp}
              label="Send (Enter)"
              variant="accent"
              size="md"
              strokeWidth={2.4}
              disabled={!canSend}
              onClick={() => void submit()}
            />
          )}
        </div>
      </div>
      <div className={styles.hint}>AI can make mistakes. Check important information.</div>
    </div>
  );
}
