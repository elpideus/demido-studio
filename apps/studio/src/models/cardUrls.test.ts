import { describe, expect, it } from 'vitest';

import { resolveCardUrl, resolveSrcSet, slug } from './cardUrls';

const repo = 'unsloth/Qwen3-8B-GGUF';

describe('resolveCardUrl', () => {
  it('points relative pictures and links at the repository on the Hub', () => {
    expect(resolveCardUrl('images/bench.png', repo, 'image')).toBe(
      'https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/images/bench.png',
    );
    expect(resolveCardUrl('./LICENSE', repo, 'link')).toBe(
      'https://huggingface.co/unsloth/Qwen3-8B-GGUF/blob/main/LICENSE',
    );
    expect(resolveCardUrl('/Qwen/Qwen3-8B', repo, 'link')).toBe('https://huggingface.co/Qwen/Qwen3-8B');
  });

  it('shows a Hub page link to a picture as the picture itself', () => {
    expect(resolveCardUrl('https://huggingface.co/Qwen/Qwen3-8B/blob/main/figures/a.png', repo, 'image')).toBe(
      'https://huggingface.co/Qwen/Qwen3-8B/resolve/main/figures/a.png',
    );
    expect(resolveCardUrl('https://huggingface.co/Qwen/Qwen3-8B/blob/main/README.md', repo, 'link')).toBe(
      'https://huggingface.co/Qwen/Qwen3-8B/blob/main/README.md',
    );
  });

  it('keeps the web, mail and anchors, and drops everything else', () => {
    expect(resolveCardUrl('//img.shields.io/badge.svg', repo, 'image')).toBe('https://img.shields.io/badge.svg');
    expect(resolveCardUrl('mailto:a@b.c', repo, 'link')).toBe('mailto:a@b.c');
    expect(resolveCardUrl('#quick-start', repo, 'link')).toBe('#quick-start');
    expect(resolveCardUrl('javascript:alert(1)', repo, 'link')).toBeNull();
    expect(resolveCardUrl('data:image/png;base64,AAAA', repo, 'image')).toBeNull();
    expect(resolveCardUrl('#x', repo, 'image')).toBeNull();
    expect(resolveCardUrl('', repo, 'image')).toBeNull();
  });
});

describe('resolveSrcSet', () => {
  it('resolves each address and keeps its descriptor', () => {
    expect(resolveSrcSet('a.png 1x, https://x.io/b.png 2x', repo)).toBe(
      'https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/a.png 1x, https://x.io/b.png 2x',
    );
  });
});

describe('slug', () => {
  it('makes anchors the way the Hub does', () => {
    expect(slug('Quick start (llama.cpp)')).toBe('quick-start-llamacpp');
    expect(slug('  Qwen3 Highlights ')).toBe('qwen3-highlights');
  });
});
