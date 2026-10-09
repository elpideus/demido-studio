// What an attached file's chip says: its type in words, the line under its name, its icon, and why
// the model may not get it the way the person expects. Pure, so the wording is tested in one place.

import { formatBytes } from '@demido/ui';

import { formatTokens } from '@/lib/format';
import type { Attachment, ModelEntry } from '@/lib/types';

/** Files one message can carry (the backend refuses more). */
export const MAX_FILES = 20;

/** Largest file that can be attached (the backend refuses larger ones too). */
export const MAX_FILE_BYTES = 100 * 1024 * 1024;

/** Window event (a CustomEvent with the paths as `detail`) that hands the composer files to attach. */
export const ATTACH_FILES_EVENT = 'demido:attach-files';

/** Which picture a file's icon tile shows; the chip maps it to a lucide icon and a tint. */
export type AttachmentIcon =
  'image' | 'audio' | 'video' | 'pdf' | 'document' | 'slides' | 'sheet' | 'data' | 'code' | 'text' | 'archive' | 'file';

type Described = Pick<Attachment, 'name' | 'mime' | 'kind'>;

// Documents, tables, code and archives by extension.
const TYPES: Record<string, [label: string, icon: AttachmentIcon]> = {
  pdf: ['PDF', 'pdf'],
  doc: ['Word document', 'document'],
  docx: ['Word document', 'document'],
  odt: ['Document', 'document'],
  rtf: ['Rich text', 'document'],
  epub: ['E-book', 'document'],
  html: ['Web page', 'document'],
  htm: ['Web page', 'document'],
  ppt: ['Presentation', 'slides'],
  pptx: ['Presentation', 'slides'],
  odp: ['Presentation', 'slides'],
  xls: ['Spreadsheet', 'sheet'],
  xlsx: ['Spreadsheet', 'sheet'],
  xlsm: ['Spreadsheet', 'sheet'],
  ods: ['Spreadsheet', 'sheet'],
  csv: ['CSV', 'sheet'],
  tsv: ['TSV', 'sheet'],
  json: ['JSON', 'data'],
  jsonl: ['JSON Lines', 'data'],
  ndjson: ['JSON Lines', 'data'],
  xml: ['XML', 'data'],
  md: ['Markdown', 'text'],
  markdown: ['Markdown', 'text'],
  txt: ['Text', 'text'],
  log: ['Log', 'text'],
  py: ['Python', 'code'],
  js: ['JavaScript', 'code'],
  mjs: ['JavaScript', 'code'],
  cjs: ['JavaScript', 'code'],
  jsx: ['JavaScript', 'code'],
  ts: ['TypeScript', 'code'],
  tsx: ['TypeScript', 'code'],
  rs: ['Rust', 'code'],
  go: ['Go', 'code'],
  java: ['Java', 'code'],
  kt: ['Kotlin', 'code'],
  swift: ['Swift', 'code'],
  c: ['C', 'code'],
  h: ['C header', 'code'],
  cpp: ['C++', 'code'],
  cc: ['C++', 'code'],
  hpp: ['C++', 'code'],
  cs: ['C#', 'code'],
  rb: ['Ruby', 'code'],
  php: ['PHP', 'code'],
  r: ['R', 'code'],
  lua: ['Lua', 'code'],
  sql: ['SQL', 'code'],
  css: ['CSS', 'code'],
  sh: ['Shell script', 'code'],
  ps1: ['PowerShell script', 'code'],
  bat: ['Batch file', 'code'],
  mq4: ['MQL4', 'code'],
  mq5: ['MQL5', 'code'],
  pine: ['Pine Script', 'code'],
  yaml: ['YAML', 'code'],
  yml: ['YAML', 'code'],
  toml: ['TOML', 'code'],
  ini: ['Config', 'code'],
  zip: ['ZIP archive', 'archive'],
  '7z': ['7z archive', 'archive'],
  rar: ['RAR archive', 'archive'],
  tar: ['TAR archive', 'archive'],
  gz: ['GZip archive', 'archive'],
};

// For files without an extension (pasted ones), what their detected type would have been.
const MIME_EXTENSIONS: Record<string, string> = {
  'application/pdf': 'pdf',
  'application/json': 'json',
  'application/zip': 'zip',
  'text/csv': 'csv',
  'text/html': 'html',
  'text/markdown': 'md',
  'text/plain': 'txt',
};

const IMAGE_FORMATS: Record<string, string> = {
  png: 'PNG',
  jpeg: 'JPEG',
  jpg: 'JPEG',
  gif: 'GIF',
  webp: 'WebP',
  bmp: 'BMP',
  tiff: 'TIFF',
  tif: 'TIFF',
  heic: 'HEIC',
  heif: 'HEIF',
  avif: 'AVIF',
  svg: 'SVG',
  icon: 'ICO',
  ico: 'ICO',
};

const AUDIO_FORMATS: Record<string, string> = {
  mpeg: 'MP3',
  mp3: 'MP3',
  wav: 'WAV',
  wave: 'WAV',
  flac: 'FLAC',
  ogg: 'OGG',
  oga: 'OGG',
  opus: 'Opus',
  mp4: 'M4A',
  m4a: 'M4A',
  aac: 'AAC',
  webm: 'WebM',
  aiff: 'AIFF',
  wma: 'WMA',
};

// What the webview can draw as a thumbnail; HEIC and TIFF get a card instead.
const PREVIEWABLE = new Set(['PNG', 'JPEG', 'GIF', 'WebP', 'BMP', 'AVIF', 'SVG', 'ICO']);

const KIND_LABELS: Record<Attachment['kind'], string> = {
  image: 'Image',
  audio: 'Audio',
  document: 'Document',
  text: 'Text',
  data: 'Data',
  other: 'File',
};

const KIND_ICONS: Record<Attachment['kind'], AttachmentIcon> = {
  image: 'image',
  audio: 'audio',
  document: 'document',
  text: 'text',
  data: 'data',
  other: 'file',
};

/** Lowercase extension without the dot; empty when there is none. */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
}

/** The last part of a path, Windows or POSIX. */
export function nameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

// "image/svg+xml" → "svg", "audio/x-wav" → "wav", "audio/vnd.wave" → "wave".
function subtype(mime: string): string {
  const sub = mime.split(';')[0]!.split('/')[1] ?? '';
  return sub
    .trim()
    .toLowerCase()
    .replace(/^(x-|vnd\.)/, '')
    .replace(/\+xml$/, '');
}

// A media format's short name: the detected type first (it reads the content), then the extension.
function mediaFormat(a: Described, table: Record<string, string>): string | null {
  const ext = extensionOf(a.name);
  return table[subtype(a.mime)] ?? table[ext] ?? (ext ? ext.toUpperCase() : null);
}

function known(a: Described): [label: string, icon: AttachmentIcon] | undefined {
  return TYPES[extensionOf(a.name)] ?? TYPES[MIME_EXTENSIONS[a.mime.split(';')[0]!.trim()] ?? ''];
}

/** "PDF", "Word document", "PNG image", "MP3 audio", "MP4 video", "EXE file". */
export function typeLabel(a: Described): string {
  if (a.kind === 'image') {
    const format = mediaFormat(a, IMAGE_FORMATS);
    return format ? `${format} image` : 'Image';
  }
  if (a.kind === 'audio') {
    const format = mediaFormat(a, AUDIO_FORMATS);
    return format ? `${format} audio` : 'Audio';
  }
  const type = known(a);
  if (type) return type[0];
  const ext = extensionOf(a.name);
  if (a.mime.startsWith('video/')) return ext ? `${ext.toUpperCase()} video` : 'Video';
  return ext ? `${ext.toUpperCase()} file` : KIND_LABELS[a.kind];
}

export function attachmentIcon(a: Described): AttachmentIcon {
  if (a.kind === 'image' || a.kind === 'audio') return a.kind;
  if (a.mime.startsWith('video/')) return 'video';
  return known(a)?.[1] ?? KIND_ICONS[a.kind];
}

/** An image the webview can draw, shown as a thumbnail rather than a card. */
export function showsThumbnail(a: Described): boolean {
  if (a.kind !== 'image') return false;
  const format = mediaFormat(a, IMAGE_FORMATS);
  return format !== null && PREVIEWABLE.has(format);
}

const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? '' : 's'}`;

/**
 * The line under a file's name: "PDF · 12 pages · 8.4K tokens", "Spreadsheet · 3 sheets",
 * "PNG image · 1920×1080", "MP3 audio · 4.2 MB".
 */
export function attachmentSubtitle(a: Attachment): string {
  const parts = [typeLabel(a)];
  if (a.kind === 'image') {
    parts.push(a.width && a.height ? `${a.width}×${a.height}` : formatBytes(a.size));
  } else if (a.kind === 'audio' || a.kind === 'other') {
    parts.push(formatBytes(a.size));
  } else {
    const icon = attachmentIcon(a);
    const unit = icon === 'sheet' ? 'sheet' : icon === 'slides' ? 'slide' : 'page';
    if (a.pages !== null) parts.push(plural(a.pages, unit));
    if (a.tokens !== null) parts.push(`${formatTokens(a.tokens)} tokens`);
    if (a.pages === null && a.tokens === null) parts.push(formatBytes(a.size));
  }
  return parts.join(' · ');
}

// A local model reads media only once llama.cpp confirmed it can (its projector loads only then),
// so anything but a yes is a no; while it is being checked nobody knows yet. A cloud model reads
// them unless its provider said it cannot.
function lacks(model: ModelEntry, can: boolean | null): boolean {
  if (model.source === 'local') return can !== true && !model.checkingCapabilities;
  return can === false;
}

// Sound formats a model is given, as the backend decides (`ModelAccess`): llama.cpp and OpenRouter
// take WAV and MP3, Gemini a few more.
const LOCAL_AUDIO = ['audio/wav', 'audio/mpeg'];
const GEMINI_AUDIO = [...LOCAL_AUDIO, 'audio/aac', 'audio/ogg', 'audio/flac', 'audio/aiff'];
const AUDIO_ALIASES: Record<string, string> = {
  'audio/x-wav': 'audio/wav',
  'audio/wave': 'audio/wav',
  'audio/vnd.wave': 'audio/wav',
  'audio/mp3': 'audio/mpeg',
  'audio/x-mp3': 'audio/mpeg',
  'audio/x-flac': 'audio/flac',
  'audio/x-aiff': 'audio/aiff',
};

/** What the person should know about a file before sending it to `model`; empty when nothing. */
export function attachmentWarnings(a: Attachment, model: ModelEntry | undefined): string[] {
  const warnings = a.note ? [a.note] : [];
  if (!model) return warnings;
  if (a.kind === 'image' && lacks(model, model.capabilities.vision)) {
    warnings.push(`${model.name} can’t see images. It gets the file in its workspace, not the picture.`);
  }
  // A voice note reaches a model that cannot hear as what was said, written down.
  if (a.kind === 'audio' && !a.voice) {
    // Sound is sent only when the model is known to hear (a cloud model too), and in its formats.
    const hears = model.capabilities.audio === true || (model.source === 'local' && model.checkingCapabilities);
    const formats = model.source === 'gemini' ? GEMINI_AUDIO : LOCAL_AUDIO;
    const mime = a.mime.split(';')[0]!.trim().toLowerCase();
    if (!hears) {
      warnings.push(`${model.name} can’t hear audio. It gets the file in its workspace, not the sound.`);
    } else if (!formats.includes(AUDIO_ALIASES[mime] ?? mime)) {
      warnings.push(
        `${model.name} can’t hear ${typeLabel(a)} files. It gets the file in its workspace, not the sound.`,
      );
    }
  }
  return warnings;
}

const pad = (n: number) => String(n).padStart(2, '0');

/**
 * A name for a pasted file. A screenshot comes without one, or as the browser's "image.png",
 * which says nothing: it becomes "Pasted image 14-03-27.png".
 */
export function pastedName(name: string, mime: string, now: Date): string {
  if (name && name !== 'image.png') return name;
  const time = `${pad(now.getHours())}-${pad(now.getMinutes())}-${pad(now.getSeconds())}`;
  if (!mime.startsWith('image/')) return name || `Pasted file ${time}`;
  const sub = subtype(mime);
  return `Pasted image ${time}.${sub === 'jpeg' ? 'jpg' : sub || 'png'}`;
}

/** An image's size scaled down to fit within `maxWidth` × `maxHeight`; null when it is unknown. */
export function fitWithin(
  width: number | null,
  height: number | null,
  maxWidth: number,
  maxHeight: number,
): { width: number; height: number } | null {
  if (!width || !height) return null;
  const scale = Math.min(1, maxWidth / width, maxHeight / height);
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}
