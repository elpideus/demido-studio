import { useEffect, useState } from 'react'
import { Route, Routes, useNavigate } from 'react-router'
import { AudioLines, Download, Mic, Play } from 'lucide-react'
import type { Feature } from '@/app/registry'
import { onCoreEvent, useCore } from '@/data/core'
import { toast, useSetting, setSetting } from '@/data/store'
import { downloadVoice, voiceStatus, type VoiceStatus } from '@/data/voice'
import { Button, Glyph, Icon, NotBuilt, Page, Section, Signal } from '@/ui'
import { LiveVoiceMode } from './live'
import { VoiceMode } from './talk'

const VOICES = [
  { id: 'none', name: 'No voice', desc: 'Demido stays silent.', by: '' },
  { id: 'ava', name: 'Ava', desc: 'Calm and clear. Good for long answers.', by: 'Open licence · may be used commercially' },
  { id: 'theo', name: 'Theo', desc: 'Lower and slower.', by: 'Open licence · may be used commercially' },
  { id: 'mila', name: 'Mila', desc: 'Bright and quick.', by: 'Non-commercial licence · personal use only' },
]
/* The Voice feature has two screens: this page, and voice mode, which takes the whole window. With a core
   behind the pages both are real; without one, the preview's. */
function VoicePage() {
  const core = useCore()
  const real = core.state === 'ready'
  return (
    <Routes>
      <Route path="talk" element={real ? <LiveVoiceMode /> : <VoiceMode />} />
      <Route path="*" element={real ? <RealVoiceHome /> : <VoiceHome />} />
    </Routes>
  )
}

const mb = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`

/** Voice, for real (DECISIONS V7): its files downloaded on demand, four donated voices, and voice mode. */
function RealVoiceHome() {
  const navigate = useNavigate()
  const [status, setStatus] = useState<VoiceStatus | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const voice = useSetting<string>('voice.real', '2181')
  const read = () => voiceStatus().then(setStatus, (e: Error) => setProblem(e.message))
  useEffect(() => { void read() }, [])
  // While a download runs, how far it has got; and when it ends, what there is.
  useEffect(() => {
    if (!status?.downloading) return
    const t = window.setInterval(() => void read(), 1000)
    return () => window.clearInterval(t)
  }, [status?.downloading])
  useEffect(() => {
    const offs = ['voice.downloaded', 'voice.download_failed'].map((k) => onCoreEvent(k, (e) => {
      void read()
      if (k === 'voice.download_failed') setProblem(String(e.data.problem))
    }))
    return () => offs.forEach((o) => o())
  }, [])
  const start = (whisper: boolean) => { setProblem(null); downloadVoice(whisper).then(setStatus, (e: Error) => setProblem(e.message)) }
  const d = status?.downloading
  return (
    <Page narrow title="Voice" lead="Talk with Demido. Speech is turned into text on this computer, and Demido's voice is made here too: nothing you say is sent away.">
      <Section title="Voice mode" hint="A conversation, hands free.">
        <div className="d-card"><div className="d-card__body app-stack">
          {!status && !problem && <p className="app-empty"><Glyph name="task-running" busy /> Reading…</p>}
          {status?.problem && <Signal level="caution">{status.problem}</Signal>}
          {status && !status.ready && !d && !status.problem && (
            <>
              <p>Voice mode needs its parts on this computer first: a speech detector, a model that hears when your sentence is finished, one that turns speech into text, and one that speaks. {mb(status.missing_bytes)}, from their makers, each checked.</p>
              <div className="app-row"><Button variant="primary" onClick={() => start(false)}><Icon icon={Download} />Download ({mb(status.missing_bytes)})</Button></div>
            </>
          )}
          {d && <p><Glyph name="task-running" busy /> Downloading {d.file || '…'}: {mb(d.done)} of {mb(d.total)}</p>}
          {status?.ready && (
            <div className="app-row"><Button variant="primary" onClick={() => navigate('/voice/talk')}><Icon icon={AudioLines} />Open voice mode</Button></div>
          )}
          <ol className="d-prose" style={{ margin: 0 }}>
            <li>Demido answers when your sentence is finished. If you stop to think, it waits; "Answer now" ends the wait.</li>
            <li>Speak while it talks and it stops at once. What you did not hear is taken out of the conversation.</li>
            <li>It speaks English. It understands 25 European languages.</li>
          </ol>
          {problem && <Signal level="caution">{problem}</Signal>}
        </div></div>
      </Section>

      <Section title="Demido's voice" hint="Voices donated by volunteers for this use (CC0), through Kyutai.">
        <div className="d-settings">
          {(status?.voices ?? []).map((v, i) => (
            <label key={v.id} className="d-setting" style={{ cursor: 'default' }}>
              <div className="d-setting__text">
                <span className="d-setting__label"><input className="d-radio" type="radio" name="voice" checked={voice === v.id} onChange={() => setSetting('voice.real', v.id)} />Voice {i + 1} <span className="d-text-tertiary">({v.id})</span></span>
                <span className="d-setting__desc">{v.present ? 'Downloaded.' : 'Downloaded with the rest.'}</span>
              </div>
              <div className="d-setting__control">{v.present && <Button variant="ghost" size="sm" onClick={(e) => { e.preventDefault(); void new Audio(`/api/v1/voice/voices/${v.id}`).play() }}><Icon icon={Play} size="sm" />Hear the speaker</Button>}</div>
            </label>
          ))}
        </div>
        <p className="d-type-caption d-text-secondary">"Hear the speaker" plays the volunteer's own recording, which Demido's voice is made from. A voice is never made from a recording of anyone who did not agree to it.</p>
      </Section>

      {status?.ready && (
        <Section title="When Demido does not catch you" hint="A second listener, for speech the first one hears nothing in.">
          <div className="d-card"><div className="d-card__body app-stack">
            <p>Parakeet writes down what you say. Now and then it hears nothing where there was speech; Whisper turbo then listens again, a second or so slower. {status.whisper ? 'It is downloaded.' : `It is ${mb(status.whisper_bytes)} more.`}</p>
            {!status.whisper && !d && <div className="app-row"><Button onClick={() => start(true)}><Icon icon={Download} />Download Whisper turbo</Button></div>}
          </div></div>
        </Section>
      )}
    </Page>
  )
}

function VoiceHome() {
  const navigate = useNavigate()
  const voice = useSetting('voice.voice', 'none')
  const [dict, setDict] = useState<'idle' | 'listening' | 'done'>('idle')
  const [consent, setConsent] = useState(false)
  const listen = () => { setDict('listening'); setTimeout(() => setDict('done'), 2400) }
  return (
    <Page narrow title="Voice" lead="Speak instead of typing, hear answers read aloud, or simply talk. Speech is turned into text on this computer; nothing is sent away.">
      <Section title="Voice mode" hint="A conversation, hands free.">
        <div className="d-card"><div className="d-card__body app-stack">
          <div className="app-row"><Button variant="primary" onClick={() => navigate('/voice/talk')}><Icon icon={AudioLines} />Open voice mode</Button><NotBuilt>Scripted: no microphone, no sound</NotBuilt></div>
          <ol className="d-prose" style={{ margin: 0 }}>
            <li>Demido answers when your sentence is finished. If you stop to think, it waits.</li>
            <li>Speak while it talks and it stops at once.</li>
            <li>Before a task that takes time, it says so first, then works.</li>
          </ol>
        </div></div>
      </Section>

      <Section title="Dictation" hint="The microphone button in every message box.">
        <div className="d-card"><div className="d-card__body app-stack">
          <div className="app-row">
            <Button variant={dict === 'listening' ? 'danger' : undefined} onClick={listen} disabled={dict === 'listening'}><Icon icon={Mic} />{dict === 'listening' ? 'Listening…' : 'Try it'}</Button>
            <NotBuilt>Scripted: your microphone is not used</NotBuilt>
          </div>
          {dict !== 'idle' && (
            <div className="app-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
              <div><p className="d-type-micro d-text-tertiary">What you said</p><p className="d-text-secondary">{dict === 'listening' ? '…' : 'ehm so can you can you clean up the build folder, no wait, the cache folder too'}</p></div>
              <div><p className="d-type-micro d-text-tertiary">What is written</p><p>{dict === 'listening' ? '…' : 'Clean up the build folder and the cache folder.'}</p></div>
            </div>
          )}
          <p className="d-type-caption d-text-secondary">Fillers, repeated words and corrections are removed. The meaning is not changed; you see the text before it is sent.</p>
        </div></div>
      </Section>

      <Section title="Demido's voice" hint="Used when an answer is read aloud and in voice mode.">
        <div className="d-settings">
          {VOICES.map((v) => (
            <label key={v.id} className="d-setting" style={{ cursor: 'default' }}>
              <div className="d-setting__text">
                <span className="d-setting__label"><input className="d-radio" type="radio" name="voice" checked={voice === v.id} onChange={() => setSetting('voice.voice', v.id)} />{v.name}</span>
                <span className="d-setting__desc">{v.desc}</span>
                {v.by && <span className="d-setting__desc d-text-tertiary">{v.by}</span>}
              </div>
              <div className="d-setting__control">{v.id !== 'none' && <Button variant="ghost" size="sm" onClick={(e) => { e.preventDefault(); toast('Playing a sample needs the voice part. Download it in Setup. (Not built in the preview.)') }}><Icon icon={Play} size="sm" />Sample</Button>}</div>
            </label>
          ))}
        </div>
      </Section>

      <Section title="Make a voice from a recording">
        <div className="d-card"><div className="d-card__body app-stack">
          <ol className="d-prose" style={{ margin: 0 }}>
            <li>Record yourself for one minute, or load audio files.</li>
            <li>Demido removes noise and music.</li>
            <li>Name the voice, give it a picture and a description.</li>
          </ol>
          <Signal level="warning">Only clone a voice that is yours, or one whose owner has agreed. A cloned voice can be used to deceive people. Demido marks cloned voices as cloned and keeps the consent you give here with the voice.</Signal>
          <label className="d-check"><input className="d-checkbox" type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} /><span className="d-check__text">This is my own voice, or I have the speaker's permission.</span></label>
          <div className="app-row"><Button disabled={!consent} onClick={() => toast('Voice cloning: not built in the preview.')}>Start recording</Button><Button disabled={!consent} variant="ghost">Load audio files</Button><NotBuilt /></div>
          <p className="d-type-caption d-text-secondary">Some cloning models may not be used commercially. Demido shows each model's licence before you download it.</p>
        </div></div>
      </Section>
    </Page>
  )
}

export const feature: Feature = {
  id: 'voice', title: 'Voice', icon: AudioLines, group: 'library', order: 3, path: 'voice', Page: VoicePage,
  immersive: ['voice/talk'],
  settings: [
    {
      id: 'voice', title: 'Voice', order: 6,
      settings: [
        { id: 'voice.cleanDictation', type: 'switch', label: 'Tidy dictated text', desc: 'Removes fillers, repeats and corrections before the text is written.', default: true },
        { id: 'voice.readAloud', type: 'switch', label: 'Read answers aloud', desc: 'Reads every answer in the chosen voice.', default: false, notBuilt: true },
        {
          id: 'voice.endOfSentence', type: 'switch', label: 'Wait until my sentence is finished', default: true,
          desc: 'In voice mode, Demido answers when what you said is a finished sentence, not at the first silence.',
          details: 'On (default): if you stop in the middle of a sentence to think, Demido stays quiet until you go on, however long that takes. "Answer now" ends the wait. Off: Demido answers after 0.7 s of silence, finished or not. Quicker, but it talks into your pauses.',
        },
        {
          id: 'voice.bargeIn', type: 'switch', label: 'Stop when I speak', default: true,
          desc: 'In voice mode, Demido stops talking the moment you speak.',
          details: 'On (default): you can interrupt. What Demido had not said yet is dropped, so it knows what you heard. Off: the microphone is closed while Demido speaks. Turn it off on loudspeakers without echo cancelling, where Demido would hear itself and stop.',
        },
        { id: 'voice.waitNotice', type: 'switch', label: 'Say "one moment" before long tasks', desc: 'In voice mode, tells you it is working before it goes quiet.', default: true },
      ],
    },
  ],
}
