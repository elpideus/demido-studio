// What an attached file's chip says: type, the line under its name, its icon and its warnings.

import { describe, expect, it } from 'vitest';
import { formatBytes } from '@demido/ui';

import type { Attachment, ModelCapabilities, ModelEntry } from '@/lib/types';
import {
  attachmentIcon,
  attachmentSubtitle,
  attachmentWarnings,
  extensionOf,
  fitWithin,
  nameFromPath,
  pastedName,
  showsThumbnail,
  typeLabel,
} from './attachmentView';

const file = (over: Partial<Attachment> = {}): Attachment => ({
  id: 'a1',
  name: 'report.pdf',
  path: 'C:\\Demido\\staging\\a1\\report.pdf',
  file: null,
  mime: 'application/pdf',
  kind: 'document',
  size: 2_400_000,
  pages: 12,
  tokens: 8400,
  width: null,
  height: null,
  note: null,
  ...over,
});

const image = (over: Partial<Attachment> = {}) =>
  file({
    name: 'chart.png',
    mime: 'image/png',
    kind: 'image',
    pages: null,
    tokens: null,
    width: 1920,
    height: 1080,
    ...over,
  });

const audio = (over: Partial<Attachment> = {}) =>
  file({ name: 'call.mp3', mime: 'audio/mpeg', kind: 'audio', size: 4_200_000, pages: null, tokens: null, ...over });

const model = (over: Partial<ModelEntry> = {}, can: Partial<ModelCapabilities> = {}): ModelEntry => ({
  id: 'local:qwen',
  source: 'local',
  providerId: null,
  providerName: null,
  name: 'Qwen 3.5 9B',
  defaultName: 'Qwen 3.5 9B',
  description: null,
  avatarPath: null,
  enabled: true,
  isDefault: true,
  path: null,
  size: null,
  quant: null,
  architecture: null,
  parameters: null,
  maxContext: null,
  repo: null,
  removable: true,
  capabilities: { vision: null, audio: null, tools: true, thinking: null, ...can },
  checkingCapabilities: false,
  settings: {},
  effective: {
    systemPrompt: '',
    temperature: null,
    topP: null,
    topK: null,
    minP: null,
    repeatPenalty: null,
    maxTokens: null,
    contextLength: null,
    gpuLayers: null,
    thinking: null,
  },
  ...over,
});

const gemini = (can: Partial<ModelCapabilities> = {}) =>
  model({ id: 'gemini:flash', source: 'gemini', name: 'Gemini Flash' }, can);

const CANT_SEE = 'Qwen 3.5 9B can’t see images. It gets the file in its workspace, not the picture.';

describe('attachmentSubtitle', () => {
  it('counts pages and tokens of what the model reads', () => {
    expect(attachmentSubtitle(file())).toBe('PDF · 12 pages · 8.4K tokens');
    expect(attachmentSubtitle(file({ name: 'plan.docx', pages: null, tokens: 3100 }))).toBe(
      'Word document · 3.1K tokens',
    );
    expect(attachmentSubtitle(file({ name: 'deck.pptx', pages: 1, tokens: 900 }))).toBe(
      'Presentation · 1 slide · 900 tokens',
    );
  });

  it('names the pages of a spreadsheet sheets', () => {
    expect(attachmentSubtitle(file({ name: 'sales.xlsx', kind: 'data', pages: 3, tokens: null }))).toBe(
      'Spreadsheet · 3 sheets',
    );
  });

  it('gives an image its pixel size and audio its file size', () => {
    expect(attachmentSubtitle(image())).toBe('PNG image · 1920×1080');
    expect(attachmentSubtitle(audio())).toBe('MP3 audio · 4.2 MB');
    expect(attachmentSubtitle(image({ width: null, height: null, size: 50_000 }))).toBe(
      `PNG image · ${formatBytes(50_000)}`,
    );
  });

  it('falls back to the file size when nothing was read', () => {
    expect(attachmentSubtitle(file({ name: 'notes.txt', kind: 'text', pages: null, tokens: null, size: 1200 }))).toBe(
      `Text · ${formatBytes(1200)}`,
    );
    expect(attachmentSubtitle(file({ name: 'setup.exe', kind: 'other', mime: 'application/x-msdownload' }))).toBe(
      `EXE file · ${formatBytes(2_400_000)}`,
    );
  });
});

describe('typeLabel', () => {
  it('trusts the detected type of an image over its extension', () => {
    expect(typeLabel(image({ name: 'photo.png', mime: 'image/jpeg' }))).toBe('JPEG image');
    expect(typeLabel(image({ name: 'logo.svg', mime: 'image/svg+xml' }))).toBe('SVG image');
    expect(typeLabel(image({ name: 'scan.tif', mime: 'application/octet-stream' }))).toBe('TIFF image');
    expect(typeLabel(audio({ name: 'memo.wav', mime: 'audio/x-wav' }))).toBe('WAV audio');
  });

  it('names documents, code and archives by their extension', () => {
    expect(typeLabel(file({ name: 'strategy.py', kind: 'text', mime: 'text/x-python' }))).toBe('Python');
    expect(typeLabel(file({ name: 'EA.mq5', kind: 'text', mime: 'text/plain' }))).toBe('MQL5');
    expect(typeLabel(file({ name: 'prices.csv', kind: 'data', mime: 'text/csv' }))).toBe('CSV');
    expect(typeLabel(file({ name: 'backup.zip', kind: 'other', mime: 'application/zip' }))).toBe('ZIP archive');
    expect(typeLabel(file({ name: 'clip.mp4', kind: 'other', mime: 'video/mp4' }))).toBe('MP4 video');
  });

  it('reads the type of a file without an extension from its detected type', () => {
    expect(typeLabel(file({ name: 'download', mime: 'application/pdf' }))).toBe('PDF');
    expect(typeLabel(file({ name: 'blob', kind: 'other', mime: 'application/octet-stream' }))).toBe('File');
  });
});

describe('attachmentIcon', () => {
  it('picks the icon by kind, then type', () => {
    expect(attachmentIcon(image())).toBe('image');
    expect(attachmentIcon(audio())).toBe('audio');
    expect(attachmentIcon(file())).toBe('pdf');
    expect(attachmentIcon(file({ name: 'sales.xlsx', kind: 'data' }))).toBe('sheet');
    expect(attachmentIcon(file({ name: 'deck.pptx' }))).toBe('slides');
    expect(attachmentIcon(file({ name: 'main.rs', kind: 'text', mime: 'text/plain' }))).toBe('code');
    expect(attachmentIcon(file({ name: 'clip.mov', kind: 'other', mime: 'video/quicktime' }))).toBe('video');
    expect(attachmentIcon(file({ name: 'data.bin', kind: 'other', mime: 'application/octet-stream' }))).toBe('file');
    expect(attachmentIcon(file({ name: 'notes', kind: 'text', mime: 'application/octet-stream' }))).toBe('text');
  });
});

describe('showsThumbnail', () => {
  it('draws only images the webview can show', () => {
    expect(showsThumbnail(image())).toBe(true);
    expect(showsThumbnail(image({ name: 'shot.webp', mime: 'image/webp' }))).toBe(true);
    expect(showsThumbnail(image({ name: 'IMG_0001.heic', mime: 'image/heic' }))).toBe(false);
    expect(showsThumbnail(file())).toBe(false);
  });
});

describe('attachmentWarnings', () => {
  it('passes on what the backend noted', () => {
    const note = 'No text found. It may be a scanned PDF.';
    expect(attachmentWarnings(file({ note }), model())).toEqual([note]);
    expect(attachmentWarnings(file(), model())).toEqual([]);
  });

  it('warns when a local model has not been confirmed to see images', () => {
    expect(attachmentWarnings(image(), model())).toEqual([CANT_SEE]);
    expect(attachmentWarnings(image(), model({}, { vision: false }))).toEqual([CANT_SEE]);
    expect(attachmentWarnings(image(), model({}, { vision: true }))).toEqual([]);
  });

  it('says nothing while llama.cpp is still checking the model', () => {
    expect(attachmentWarnings(image(), model({ checkingCapabilities: true }))).toEqual([]);
  });

  it('warns about a cloud model only when its provider said no', () => {
    expect(attachmentWarnings(image(), gemini())).toEqual([]);
    expect(attachmentWarnings(image(), gemini({ vision: false }))).toEqual([
      'Gemini Flash can’t see images. It gets the file in its workspace, not the picture.',
    ]);
  });

  it('does the same for audio, in the formats the model is given sound in', () => {
    expect(attachmentWarnings(audio(), model({}, { vision: true }))).toEqual([
      'Qwen 3.5 9B can’t hear audio. It gets the file in its workspace, not the sound.',
    ]);
    expect(attachmentWarnings(audio(), model({}, { audio: true }))).toEqual([]);
    expect(attachmentWarnings(audio({ name: 'memo.wav', mime: 'audio/x-wav' }), model({}, { audio: true }))).toEqual(
      [],
    );
    // llama.cpp reads WAV and MP3 only; Gemini also FLAC.
    const flac = audio({ name: 'take.flac', mime: 'audio/flac' });
    expect(attachmentWarnings(flac, model({}, { audio: true }))).toEqual([
      'Qwen 3.5 9B can’t hear FLAC audio files. It gets the file in its workspace, not the sound.',
    ]);
    expect(attachmentWarnings(flac, gemini({ audio: true }))).toEqual([]);
    // A cloud model is given sound only when it is known to hear.
    expect(attachmentWarnings(audio(), gemini())).toEqual([
      'Gemini Flash can’t hear audio. It gets the file in its workspace, not the sound.',
    ]);
  });

  it('lists a note and the model warning together, and only the note without a model', () => {
    const note = 'This image could not be read.';
    expect(attachmentWarnings(image({ note }), model())).toEqual([note, CANT_SEE]);
    expect(attachmentWarnings(image({ note }), undefined)).toEqual([note]);
  });
});

describe('pastedName', () => {
  const at = new Date(2026, 8, 30, 9, 5, 3);

  it('names a screenshot by the time it was pasted', () => {
    expect(pastedName('image.png', 'image/png', at)).toBe('Pasted image 09-05-03.png');
    expect(pastedName('', 'image/png', at)).toBe('Pasted image 09-05-03.png');
    expect(pastedName('', 'image/jpeg', at)).toBe('Pasted image 09-05-03.jpg');
    expect(pastedName('', '', at)).toBe('Pasted file 09-05-03');
  });

  it('keeps the name of a pasted file', () => {
    expect(pastedName('report.pdf', 'application/pdf', at)).toBe('report.pdf');
    expect(pastedName('holiday.png', 'image/png', at)).toBe('holiday.png');
  });
});

describe('paths and sizes', () => {
  it('takes the name from a Windows or POSIX path', () => {
    expect(nameFromPath('C:\\Users\\me\\Documents\\report.pdf')).toBe('report.pdf');
    expect(nameFromPath('/home/me/notes.md')).toBe('notes.md');
  });

  it('reads the last extension, and none from a dotfile', () => {
    expect(extensionOf('backup.tar.GZ')).toBe('gz');
    expect(extensionOf('.env')).toBe('');
    expect(extensionOf('README')).toBe('');
  });

  it('scales an image down to fit, never up', () => {
    expect(fitWithin(1920, 1080, 260, 320)).toEqual({ width: 260, height: 146 });
    expect(fitWithin(1080, 1920, 260, 320)).toEqual({ width: 180, height: 320 });
    expect(fitWithin(100, 50, 260, 320)).toEqual({ width: 100, height: 50 });
    expect(fitWithin(null, 50, 260, 320)).toBeNull();
  });
});
