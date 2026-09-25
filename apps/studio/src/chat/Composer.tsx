import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { IconButton, Spinner, TextArea } from '@demido/ui';

import type { ModelEntry } from '@/lib/types';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { ModelPicker } from './ModelPicker';
import { ToolsPicker } from './ToolsPicker';
import styles from './Composer.module.css';

interface Props {
  chatId: string | null;
  model: ModelEntry | undefined;
  /** Text to put in the composer (from a suggestion), then cleared by the parent. */
  prefill?: string | null;
}

export function Composer({ chatId, model, prefill }: Props) {
  const [text, setText] = useState('');
  const ref = useRef<HTMLTextAreaElement>(null);
  const running = useChats((s) => (chatId ? !!s.running[chatId] : false));
  const send = useChats((s) => s.send);
  const stop = useChats((s) => s.stop);
  const pickModel = useChats((s) => s.pickModel);
  const runtime = useModels((s) => s.runtime);

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

  const submit = async () => {
    const value = text.trim();
    if (!value || !model || running) return;
    setText('');
    const ok = await send(value, model.id);
    if (!ok) setText(value);
  };

  const loading = runtime?.state === 'loading' && model && runtime.modelId === model.id;
  const loadError = runtime?.state === 'error' && model && runtime.modelId === model.id ? runtime.message : null;

  return (
    <div className={styles.wrap}>
      {loading && (
        <div className={styles.notice}>
          <Spinner size={12} /> Loading {model.name} into memory…
        </div>
      )}
      {loadError && <div className={styles.noticeError}>{loadError}</div>}
      <div className={styles.composer}>
        <TextArea
          ref={ref}
          className={styles.input}
          autoSize={{ min: 1, max: 12 }}
          value={text}
          placeholder={model ? `Message ${model.name}` : 'Choose a model to start'}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void submit();
            }
          }}
          aria-label="Message"
          rows={1}
        />
        <div className={styles.toolbar}>
          <ModelPicker model={model} onPick={pickModel} />
          <ToolsPicker />
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
              disabled={!text.trim() || !model}
              onClick={() => void submit()}
            />
          )}
        </div>
      </div>
      <div className={styles.hint}>AI can make mistakes. Check important information.</div>
    </div>
  );
}
