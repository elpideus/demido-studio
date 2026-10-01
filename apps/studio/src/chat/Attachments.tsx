import { useState, type CSSProperties } from 'react';
import {
  FileArchive,
  FileBraces,
  FileCode,
  FileIcon,
  FileImage,
  FileMusic,
  FileSpreadsheet,
  FileText,
  FileVideoCamera,
  Presentation,
  TriangleAlert,
  X,
  type LucideIcon,
} from 'lucide-react';
import { Spinner, Tooltip, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl } from '@/lib/format';
import type { Attachment, ModelEntry } from '@/lib/types';
import { toast } from '@/stores/toasts';
import {
  attachmentIcon,
  attachmentSubtitle,
  attachmentWarnings,
  fitWithin,
  showsThumbnail,
  type AttachmentIcon,
} from './attachmentView';
import styles from './Attachments.module.css';

const ICONS: Record<AttachmentIcon, LucideIcon> = {
  image: FileImage,
  audio: FileMusic,
  video: FileVideoCamera,
  pdf: FileText,
  document: FileText,
  slides: Presentation,
  sheet: FileSpreadsheet,
  data: FileBraces,
  code: FileCode,
  text: FileText,
  archive: FileArchive,
  file: FileIcon,
};

/** A file in the composer: read by the backend, or still being read. */
export interface StagedFile {
  /** Stable while the chip is on screen; not the attachment's id, which arrives once it is read. */
  key: string;
  name: string;
  /** Null while the backend reads the file. */
  attachment: Attachment | null;
}

function openAttachment(a: Attachment) {
  api.openPath(a.path).catch((e) => toast.error(`Could not open ${a.name}`, errorText(e)));
}

/** A triangle whose tooltip says what the person should know about a file. */
function Warning({ warnings, className, focusable }: { warnings: string[]; className?: string; focusable?: boolean }) {
  return (
    <Tooltip
      className={className}
      content={
        <div className={styles.warningText}>
          {warnings.map((w) => (
            <p key={w}>{w}</p>
          ))}
        </div>
      }
    >
      <span role="img" aria-label={warnings.join(' ')} tabIndex={focusable ? 0 : undefined} className={styles.warning}>
        <TriangleAlert size={14} strokeWidth={2} aria-hidden />
      </span>
    </Tooltip>
  );
}

/** An image from disk; a plain tile when the webview cannot draw it. */
function Picture({
  attachment,
  className,
  style,
}: {
  attachment: Attachment;
  className: string | undefined;
  style?: CSSProperties;
}) {
  const [failed, setFailed] = useState(false);
  const src = fileUrl(attachment.path);
  if (failed || !src) {
    return (
      <span className={cx(className, styles.broken)} style={style}>
        <FileImage size={20} strokeWidth={1.6} aria-hidden />
      </span>
    );
  }
  return (
    <img
      className={className}
      style={style}
      src={src}
      alt={attachment.name}
      loading="lazy"
      decoding="async"
      draggable={false}
      onError={() => setFailed(true)}
    />
  );
}

/** A file as a card: its kind, name and what the model gets from it. */
function FileCard({
  name,
  attachment,
  warnings,
  focusableWarning,
  onOpen,
}: {
  name: string;
  /** Null while the file is read. */
  attachment: Attachment | null;
  warnings: string[];
  focusableWarning?: boolean;
  onOpen?: () => void;
}) {
  const icon = attachment ? attachmentIcon(attachment) : null;
  const Icon = icon ? ICONS[icon] : null;
  const body = (
    <>
      <span className={styles.tile} data-icon={icon ?? undefined}>
        {Icon ? <Icon size={18} strokeWidth={1.7} aria-hidden /> : <Spinner size={14} />}
      </span>
      <span className={styles.text}>
        <span className={styles.name}>{name}</span>
        <span className={styles.subtitle}>{attachment ? attachmentSubtitle(attachment) : 'Reading…'}</span>
      </span>
      {warnings.length > 0 && <Warning warnings={warnings} focusable={focusableWarning} />}
    </>
  );
  return onOpen ? (
    <button type="button" className={cx(styles.card, styles.openable)} onClick={onOpen}>
      {body}
    </button>
  ) : (
    <div className={styles.card}>{body}</div>
  );
}

/** The composer's files, above the text: thumbnails for images, cards for the rest. */
export function AttachmentTray({
  files,
  model,
  onRemove,
}: {
  files: StagedFile[];
  model: ModelEntry | undefined;
  onRemove: (key: string) => void;
}) {
  return (
    <ul className={styles.tray} aria-label="Attached files">
      {files.map(({ key, name, attachment }) => {
        const warnings = attachment ? attachmentWarnings(attachment, model) : [];
        return (
          <li key={key} className={styles.item}>
            {attachment && showsThumbnail(attachment) ? (
              <span className={styles.thumb}>
                <Picture attachment={attachment} className={styles.thumbImage} />
                {warnings.length > 0 && <Warning warnings={warnings} className={styles.thumbWarning} focusable />}
              </span>
            ) : (
              <FileCard name={name} attachment={attachment} warnings={warnings} focusableWarning />
            )}
            <button type="button" className={styles.remove} aria-label={`Remove ${name}`} onClick={() => onRemove(key)}>
              <X size={12} strokeWidth={2.4} aria-hidden />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

/** A sent message's files, above its bubble; each opens in its own app. */
export function MessageAttachments({ attachments }: { attachments: Attachment[] }) {
  const images = attachments.filter(showsThumbnail);
  const others = attachments.filter((a) => !showsThumbnail(a));
  const single = images.length === 1 ? images[0] : undefined;
  // A lone image keeps its shape, so the list does not jump when it loads.
  const size = single ? fitWithin(single.width, single.height, 260, 320) : null;
  const shape = size ? { width: size.width, aspectRatio: `${size.width} / ${size.height}` } : undefined;
  return (
    <div className={styles.sent} role="group" aria-label="Attached files">
      {images.length > 0 && (
        <ul className={cx(styles.gallery, single && styles.gallerySingle)}>
          {images.map((a) => (
            <li key={a.id}>
              <button
                type="button"
                className={styles.imageButton}
                aria-label={`Open ${a.name}`}
                onClick={() => openAttachment(a)}
              >
                <Picture attachment={a} className={single ? styles.singleImage : styles.gridImage} style={shape} />
              </button>
            </li>
          ))}
        </ul>
      )}
      {others.length > 0 && (
        <ul className={styles.files}>
          {others.map((a) => (
            <li key={a.id}>
              <FileCard
                name={a.name}
                attachment={a}
                warnings={attachmentWarnings(a, undefined)}
                onOpen={() => openAttachment(a)}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
