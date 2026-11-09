import { useCallback, useEffect, useState } from 'react';
import { Download } from 'lucide-react';
import { Button, Select, Switch, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { SpeechChoice } from '@/lib/types';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import { microphones, unlockNames } from '@/voice/recorder';
import { Group, PageHeader, Row } from './layout';
import s from './settings.module.css';

/** The microphone, the speech model that writes down what is said, and where the voice goes. */
export function VoiceTab() {
  const settings = useApp((st) => st.settings);
  const patch = useApp((st) => st.patchSettings);
  const [mics, setMics] = useState<MediaDeviceInfo[]>([]);
  const [speech, setSpeech] = useState<SpeechChoice[]>([]);

  const findMics = useCallback(() => {
    microphones().then(setMics, () => setMics([]));
  }, []);

  useEffect(() => {
    findMics();
    navigator.mediaDevices?.addEventListener?.('devicechange', findMics);
    return () => navigator.mediaDevices?.removeEventListener?.('devicechange', findMics);
  }, [findMics]);

  useEffect(() => {
    const load = () => void api.speechModels().then(setSpeech, () => undefined);
    load();
    // A finished download installs a speech model.
    const unlisten = on('downloads://changed', (job) => {
      if (job.state === 'done') load();
    });
    return () => void unlisten.then((f) => f());
  }, []);

  if (!settings) return null;

  // Microphones have names only once the app has used one; asking for one names them all.
  const unnamed = mics.some((m) => !m.label);
  const nameMics = () => {
    if (!unnamed) return;
    unlockNames().then(findMics, () => undefined);
  };

  const chosenMic = settings.microphone ?? '';
  // Before the app has used a microphone, the browser lists them without ids as well: none can
  // be picked, and each would be another "System default".
  const micOptions = [
    { value: '', label: 'System default' },
    ...mics.filter((m) => m.deviceId).map((m, i) => ({ value: m.deviceId, label: m.label || `Microphone ${i + 1}` })),
  ];
  if (chosenMic && !mics.some((m) => m.deviceId === chosenMic)) {
    micOptions.push({ value: chosenMic, label: 'A microphone not plugged in' });
  }

  const auto = speech.find((m) => m.forThisComputer);
  const chosen = speech.find((m) => m.id === settings.speechModel) ?? auto;
  const speechOptions = [
    { value: '', label: auto ? `Automatic (${auto.name})` : 'Automatic' },
    ...speech.map((m) => ({ value: m.id, label: m.installed ? m.name : `${m.name} · not downloaded` })),
  ];

  const download = () =>
    api.downloadSpeechModel().then(
      (job) => toast.info(`Downloading ${job.name}`, 'Settings, Models shows how far it got.'),
      (e) => toast.error('Could not download the speech model', errorText(e)),
    );

  return (
    <div className={s.page}>
      <PageHeader page="voice" />
      <div className={s.scroll}>
        <Group
          title="Input"
          footer="Click the microphone in the message box to talk instead of typing. Esc cancels a recording."
        >
          <Row setting="microphone" label="Microphone">
            <Select
              size="sm"
              aria-label="Microphone"
              className={s.wide}
              value={chosenMic}
              onPointerDown={nameMics}
              onFocus={nameMics}
              onChange={(e) => void patch({ microphone: e.target.value || null })}
              options={micOptions}
            />
          </Row>
        </Group>

        <Group title="Understanding what you say">
          <Row
            setting="speech-model"
            label="Speech model"
            description={
              chosen && !chosen.installed
                ? `Writes down what you say for models that can’t hear. ${chosen.name} is not downloaded yet (${formatBytes(chosen.size)}).`
                : 'Writes down what you say for models that can’t hear, on this computer. Automatic picks by your graphics memory; 1.7B is more accurate and needs more.'
            }
          >
            {chosen && !chosen.installed && (
              <Button size="sm" variant="secondary" icon={Download} onClick={() => void download()}>
                Download
              </Button>
            )}
            <Select
              size="sm"
              aria-label="Speech model"
              className={s.wide}
              value={settings.speechModel ?? ''}
              onChange={(e) => void patch({ speechModel: e.target.value || null })}
              options={speechOptions}
            />
          </Row>
          <Row
            setting="send-voice"
            label="Send my voice to models that can hear"
            description="They get the recording itself, tone and all. Off, every model gets what you said written down instead."
          >
            <Switch
              checked={settings.sendVoice ?? true}
              onChange={(v) => void patch({ sendVoice: v })}
              label="Send my voice to models that can hear"
            />
          </Row>
        </Group>
      </div>
    </div>
  );
}
