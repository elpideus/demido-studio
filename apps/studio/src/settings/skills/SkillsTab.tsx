import { useEffect, useState } from 'react';
import {
  ArrowLeft,
  Blocks,
  Bot,
  FileText,
  FolderOpen,
  Pencil,
  Plus,
  Save,
  Trash2,
} from 'lucide-react';
import { Badge, Button, Dialog, EmptyState, Field, Notice, Switch, TextArea, TextField, cx, formatBytes } from '@demido/ui';

import { Markdown } from '@/chat/Markdown';
import { api, errorText } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { Skill } from '@/lib/types';
import { useSkills } from '@/stores/skills';
import { toast } from '@/stores/toasts';
import { type WindowState, useWindows } from '@/stores/windows';
import s from '../settings.module.css';
import styles from './Skills.module.css';

function NewSkillDialog({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (s: Skill) => void }) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [instructions, setInstructions] = useState('');
  const [busy, setBusy] = useState(false);
  const create = async () => {
    setBusy(true);
    try {
      const skill = await api.createSkill(name, description, instructions);
      onCreated(skill);
      setName('');
      setDescription('');
      setInstructions('');
      onClose();
    } catch (e) {
      toast.error('Could not create the skill', errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={open}
      onClose={onClose}
      width={620}
      title="New skill"
      description="Instructions the assistant follows whenever a request matches. You can also ask the assistant to write one for you."
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!name.trim() || !instructions.trim()} onClick={() => void create()}>
            Create
          </Button>
        </>
      }
    >
      <div className={s.stack}>
        <Field label="Name">
          <TextField value={name} placeholder="Weekly market recap" onChange={(e) => setName(e.target.value)} autoFocus />
        </Field>
        <Field label="When to use it" description="One sentence. The assistant reads this to decide when the skill applies.">
          <TextField value={description} placeholder="When the user asks for a weekly recap of a market" onChange={(e) => setDescription(e.target.value)} />
        </Field>
        <Field label="Instructions" description="Markdown. Numbered steps work best.">
          <TextArea
            mono
            autoSize={{ min: 8, max: 18 }}
            value={instructions}
            placeholder={'1. Get the last 5 daily candles with market_candles.\n2. ...'}
            onChange={(e) => setInstructions(e.target.value)}
          />
        </Field>
      </div>
    </Dialog>
  );
}

function SkillDetail({ skill, onBack }: { skill: Skill; onBack: () => void }) {
  const toggle = useSkills((st) => st.toggleSkill);
  const [file, setFile] = useState('SKILL.md');
  const [content, setContent] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    setContent(null);
    setEditing(false);
    api.readSkillFile(skill.id, file).then(setContent, (e) => setContent(`Could not read this file: ${errorText(e)}`));
  }, [skill.id, file, skill.updatedAt]);

  const isMarkdown = file.toLowerCase().endsWith('.md');
  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <button type="button" className={s.back} onClick={onBack}>
            <ArrowLeft size={14} aria-hidden /> Skills
          </button>
          <h2 className={s.title}>{skill.name}</h2>
          <p className={s.subtitle}>{skill.description}</p>
        </div>
        <div className={s.headerActions}>
          <Switch checked={skill.enabled} onChange={(v) => void toggle(skill.id, v)} label="Enabled" />
        </div>
      </header>
      <div className={styles.detail}>
        <nav className={styles.files} aria-label="Files">
          <div className={styles.filesTitle}>Files</div>
          {skill.files.map((f) => (
            <button
              key={f.path}
              type="button"
              className={cx(styles.file, f.path === file && styles.fileActive)}
              onClick={() => setFile(f.path)}
              title={`${f.path} · ${formatBytes(f.size)}`}
            >
              <FileText size={14} aria-hidden />
              <span>{f.path}</span>
            </button>
          ))}
          <div className={styles.fileMeta}>
            {skill.author === 'assistant' && (
              <Badge icon={Bot} tone="info">
                Written by the assistant
              </Badge>
            )}
            <span className={s.muted}>Changed {formatDateTime(skill.updatedAt)}</span>
          </div>
          <div className={styles.fileActions}>
            <Button size="sm" variant="secondary" icon={FolderOpen} onClick={() => void api.openSkillsFolder(skill.id)}>
              Open folder
            </Button>
            <Button size="sm" variant="ghost" icon={Trash2} onClick={() => setDeleting(true)}>
              Delete
            </Button>
          </div>
        </nav>
        <div className={styles.viewer}>
          <div className={styles.viewerBar}>
            <span className={s.mono}>{file}</span>
            {!editing ? (
              <Button
                size="sm"
                variant="ghost"
                icon={Pencil}
                disabled={content === null}
                onClick={() => {
                  setDraft(content ?? '');
                  setEditing(true);
                }}
              >
                Edit
              </Button>
            ) : (
              <div className={styles.editActions}>
                <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                  Cancel
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  icon={Save}
                  onClick={() =>
                    void api.writeSkillFile(skill.id, file, draft).then(
                      () => {
                        setContent(draft);
                        setEditing(false);
                        toast.success('Saved', 'The assistant uses the new version from the next message.');
                      },
                      (e) => toast.error('Could not save', errorText(e)),
                    )
                  }
                >
                  Save
                </Button>
              </div>
            )}
          </div>
          <div className={styles.viewerBody}>
            {editing ? (
              <textarea className={styles.editor} value={draft} onChange={(e) => setDraft(e.target.value)} spellCheck={false} />
            ) : content === null ? null : isMarkdown ? (
              <Markdown text={content} />
            ) : (
              <pre className={styles.code}>{content}</pre>
            )}
          </div>
        </div>
      </div>
      <Dialog
        open={deleting}
        onClose={() => setDeleting(false)}
        title={`Delete “${skill.name}”?`}
        description="The skill's folder and all its files are deleted."
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(false)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() =>
                void api.deleteSkill(skill.id).then((list) => {
                  useSkills.getState().setSkills(list);
                  setDeleting(false);
                  onBack();
                })
              }
            >
              Delete
            </Button>
          </>
        }
      />
    </div>
  );
}

export function SkillsTab({ win }: { win: WindowState }) {
  const skills = useSkills((st) => st.skills);
  const toggle = useSkills((st) => st.toggleSkill);
  const setProps = useWindows((st) => st.setProps);
  const [creating, setCreating] = useState(false);
  const selectedId = typeof win.props.skill === 'string' ? win.props.skill : null;
  const selected = skills.find((sk) => sk.id === selectedId);

  if (selected) return <SkillDetail skill={selected} onBack={() => setProps(win.id, { skill: undefined })} />;

  return (
    <div className={s.page}>
      <header className={s.header}>
        <div className={s.headerText}>
          <h2 className={s.title}>Skills</h2>
          <p className={s.subtitle}>
            Saved procedures the assistant follows when a request matches. Enabled skills are part of every conversation.
          </p>
        </div>
        <div className={s.headerActions}>
          <Button variant="ghost" icon={FolderOpen} onClick={() => void api.openSkillsFolder()}>
            Folder
          </Button>
          <Button icon={Plus} onClick={() => setCreating(true)}>
            New skill
          </Button>
        </div>
      </header>
      <div className={s.scroll}>
        {skills.length === 0 ? (
          <div className={s.card}>
            <EmptyState
              icon={Blocks}
              title="No skills yet"
              description="Create one here, drop a folder with a SKILL.md into the skills folder, or ask the assistant to turn a task into a skill."
              action={
                <Button variant="primary" icon={Plus} onClick={() => setCreating(true)}>
                  New skill
                </Button>
              }
            />
          </div>
        ) : (
          <div className={s.rows}>
            {skills.map((sk) => (
              <div
                key={sk.id}
                className={`${s.row} ${s.rowButton}`}
                role="button"
                tabIndex={0}
                onClick={() => setProps(win.id, { skill: sk.id })}
                onKeyDown={(e) => e.key === 'Enter' && setProps(win.id, { skill: sk.id })}
              >
                <span className={styles.skillIcon}>
                  <Blocks size={18} strokeWidth={1.8} aria-hidden />
                </span>
                <div className={s.rowMain}>
                  <div className={s.rowTitle}>
                    {sk.name}
                    {sk.author === 'assistant' && (
                      <Badge icon={Bot} tone="info">
                        Assistant
                      </Badge>
                    )}
                    {sk.problem && <Badge tone="danger">Broken</Badge>}
                  </div>
                  <div className={s.rowMeta}>{sk.problem ?? sk.description}</div>
                </div>
                <span className={s.muted}>
                  {sk.files.length} {sk.files.length === 1 ? 'file' : 'files'}
                </span>
                <div className={s.rowActions} onClick={(e) => e.stopPropagation()}>
                  <Switch checked={sk.enabled} disabled={!!sk.problem} onChange={(v) => void toggle(sk.id, v)} label={sk.name} />
                </div>
              </div>
            ))}
          </div>
        )}
        <Notice tone="info" icon={Blocks} className={styles.notice}>
          A skill is a folder with a <code>SKILL.md</code> file (a short frontmatter with <code>name</code> and{' '}
          <code>description</code>, then instructions) and any other files it refers to. Changes in the folder appear
          here immediately.
        </Notice>
      </div>
      <NewSkillDialog open={creating} onClose={() => setCreating(false)} onCreated={(sk) => setProps(win.id, { skill: sk.id })} />
    </div>
  );
}
