// Runs in a browser tab's page for the assistant: reads the page as text, with a numbered
// reference for everything that can be clicked or typed in, and clicks, types and scrolls by
// those references (or a CSS selector). The app evaluates it afresh for every call as
// `(this file)(action, args)`; nothing stays in the page but the references (`data-demido-ref`
// attributes, so a reference keeps its number from one reading to the next), a counter of
// changes to the page, which tells the app when a page has settled after a click, and the
// outcomes of the assistant's own scripts until the app collects them. It returns a plain
// object, which the app receives as JSON. Synchronous on purpose: neither WebKitGTK nor WebView2
// hands back a promise. A script of the assistant's comes as a third argument, an async function
// (`run`), whose outcome the app asks for afterwards (`ran`).
(function (action, a, code) {
  'use strict';
  const REF = 'data-demido-ref';
  const MAX_TEXT = 400000;
  const MAX_HTML = 4 * 1024 * 1024;
  // Elements without text for a reader.
  const SKIP = new Set(['script', 'style', 'noscript', 'template', 'svg', 'canvas', 'head', 'object', 'embed']);
  // Elements that start a line of their own.
  const BLOCK = new Set([
    'address',
    'article',
    'aside',
    'blockquote',
    'caption',
    'dd',
    'details',
    'dialog',
    'div',
    'dl',
    'dt',
    'fieldset',
    'figcaption',
    'figure',
    'footer',
    'form',
    'h1',
    'h2',
    'h3',
    'h4',
    'h5',
    'h6',
    'header',
    'hr',
    'li',
    'main',
    'nav',
    'ol',
    'p',
    'pre',
    'section',
    'table',
    'tbody',
    'tfoot',
    'thead',
    'tr',
    'ul',
  ]);
  // ARIA roles of controls.
  const ROLES = new Set([
    'button',
    'link',
    'checkbox',
    'radio',
    'switch',
    'tab',
    'menuitem',
    'menuitemcheckbox',
    'menuitemradio',
    'option',
    'combobox',
    'searchbox',
    'textbox',
    'slider',
    'spinbutton',
    'treeitem',
  ]);
  // Why the assistant does not type in a field.
  const SECRET_FIELD =
    'asks for a password or payment details, which only the user types: ask them with browser_ask_user.';
  const SECRET = 'The field with the keyboard ' + SECRET_FIELD;
  // What holds a control of its own, so is not one control as a whole.
  const CONTROLS = [
    'a[href]',
    'button',
    'summary',
    'select',
    'textarea',
    'input:not([type="hidden"])',
    '[contenteditable=""]',
    '[contenteditable="true"]',
    '[onclick]',
    '[tabindex]:not([tabindex^="-"])',
    ...Array.from(ROLES, (r) => '[role="' + r + '"]'),
  ].join(',');

  try {
    switch (action) {
      case 'state':
        return state();
      case 'snapshot':
        return snapshot();
      case 'html':
        return html();
      case 'bytes':
        return bytes(a);
      case 'click':
        return click(a);
      case 'type':
        return type(a);
      case 'scroll':
        return scroll(a);
      case 'at':
        return at(a);
      case 'clickAt':
        return clickAt(a);
      case 'focused':
        return focused();
      case 'insert':
        return insert(a);
      case 'press':
        return press(a);
      case 'aim':
        return aim(a);
      case 'focus':
        return focus(a);
      case 'view':
        return view();
      case 'marks':
        return marks(a);
      case 'keepFocus':
        return keepFocus();
      case 'run':
        return run(a, code);
      case 'ran':
        return ran(a);
      case 'parse':
        return parse(a);
      case 'back':
        history.back();
        return { ok: true };
      case 'forward':
        history.forward();
        return { ok: true };
      case 'stop':
        window.stop();
        return { ok: true };
      default:
        return { error: 'Unknown action ' + action };
    }
  } catch (e) {
    return { error: String((e && e.message) || e) };
  }

  // Where the page is, how many changes it has seen, and, for a small page, its HTML, in which
  // the app looks for a bot check.
  function state() {
    if (!window.__demidoChanges) {
      const counter = { n: 0 };
      Object.defineProperty(window, '__demidoChanges', { value: counter, enumerable: false });
      new MutationObserver((list) => {
        counter.n += list.length;
      }).observe(document, { subtree: true, childList: true, characterData: true, attributes: true });
    }
    const small = document.getElementsByTagName('*').length < 1500;
    return {
      url: location.href,
      title: document.title,
      ready: document.readyState,
      changes: window.__demidoChanges.n,
      textLength: document.body ? document.body.innerText.length : 0,
      small: small ? document.documentElement.outerHTML.slice(0, 200000) : null,
    };
  }

  function html() {
    const page = document.documentElement ? document.documentElement.outerHTML : '';
    return { url: location.href, title: document.title, type: document.contentType, html: page.slice(0, MAX_HTML) };
  }

  // The page's own file, fetched again with the tab's cookies: for a document the browser shows in
  // a viewer of its own (a PDF), whose text is not in the page. As base64, at most `a.max` bytes.
  // Not used on Linux, where WebKitGTK hands over the file it loaded. A synchronous request, as
  // this file must be; a binary charset keeps each byte one character.
  function bytes(a) {
    const xhr = new XMLHttpRequest();
    xhr.open('GET', location.href, false);
    xhr.overrideMimeType('text/plain; charset=x-user-defined');
    xhr.send();
    if (xhr.status < 200 || xhr.status >= 300) return { error: 'The file answered ' + xhr.status + '.' };
    const raw = xhr.responseText;
    const n = Math.min(raw.length, a.max || raw.length);
    // Pieces a multiple of 3 bytes long encode to base64 that joins up.
    const PIECE = 3 * 8192;
    const parts = [];
    for (let i = 0; i < n; i += PIECE) {
      const codes = [];
      for (let j = i; j < Math.min(i + PIECE, n); j++) codes.push(raw.charCodeAt(j) & 0xff);
      parts.push(btoa(String.fromCharCode.apply(null, codes)));
    }
    return {
      type: xhr.getResponseHeader('content-type') || document.contentType,
      base64: parts.join(''),
      size: raw.length,
    };
  }

  // ---- Reading ---------------------------------------------------------------------------

  function snapshot() {
    let next = window.__demidoRefs || 0;
    const lines = [];
    let line = '';
    let refs = 0;

    const flush = () => {
      const text = line.replace(/\s+/g, ' ').trim();
      if (text) lines.push(text);
      line = '';
    };
    const push = (text) => {
      line += text;
    };
    const refOf = (el) => {
      let r = el.getAttribute(REF);
      if (!r) {
        next += 1;
        r = String(next);
        el.setAttribute(REF, r);
      }
      refs += 1;
      return r;
    };

    walk(document.body, 0, false);
    flush();
    Object.defineProperty(window, '__demidoRefs', {
      value: next,
      writable: true,
      configurable: true,
      enumerable: false,
    });

    let text = lines.join('\n').replace(/\n{3,}/g, '\n\n');
    if (text.length > MAX_TEXT) text = text.slice(0, MAX_TEXT);
    return {
      url: location.href,
      title: document.title,
      type: document.contentType,
      text,
      refs,
      scrollY: Math.round(window.scrollY),
      scrollHeight: Math.round(document.documentElement.scrollHeight),
      viewportHeight: window.innerHeight,
    };

    // `inHand` says the pointer turns into a hand over the parent, which was not taken as one
    // control: each of its items is judged on its own then, but not each piece of its text.
    function walk(node, depth, inHand) {
      if (!node || depth > 200) return;
      if (node.nodeType === Node.TEXT_NODE) {
        push(node.textContent);
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE) return;
      const el = node;
      const tag = el.tagName.toLowerCase();
      if (SKIP.has(tag)) return;
      const style = getComputedStyle(el);
      if (!shown(el, style)) return;
      const hand = style.cursor === 'pointer';
      if (el.shadowRoot) {
        for (const child of el.shadowRoot.childNodes) walk(child, depth + 1, hand);
      }
      if (tag === 'iframe' || tag === 'frame') {
        frame(el);
        return;
      }
      const kind =
        interactive(el, tag) || (hand && !(inHand && style.display === 'inline') && pointer(el, tag) ? 'button' : '');
      if (kind) {
        flushIfBlock(tag);
        push(' ' + describe(el, tag, kind, refOf(el)) + ' ');
        if (BLOCK.has(tag)) flush();
        return;
      }
      const block = BLOCK.has(tag);
      if (block) flush();
      if (/^h[1-6]$/.test(tag)) push('#'.repeat(+tag[1]) + ' ');
      else if (tag === 'li') push('- ');
      else if (tag === 'img') {
        const alt = (el.getAttribute('alt') || '').trim();
        if (alt) push(' [image: ' + alt + '] ');
        return;
      } else if (tag === 'br') {
        flush();
        return;
      } else if (tag === 'pre') {
        flush();
        for (const l of el.innerText.split('\n')) lines.push('    ' + l);
        return;
      }
      for (const child of el.childNodes) walk(child, depth + 1, hand);
      if (tag === 'td' || tag === 'th') push(' | ');
      if (block) flush();
    }

    function flushIfBlock(tag) {
      if (BLOCK.has(tag)) flush();
    }

    function frame(el) {
      let doc = null;
      try {
        doc = el.contentDocument;
      } catch (e) {
        doc = null;
      }
      if (doc && doc.body) {
        flush();
        walk(doc.body, 1, false);
        flush();
        return;
      }
      const src = el.getAttribute('src') || '';
      const title = el.getAttribute('title') || '';
      if (/captcha|challenge|turnstile/i.test(src + ' ' + title)) {
        flush();
        lines.push('[a captcha or bot check the assistant cannot see into: ' + (title || src) + ']');
      }
    }
  }

  // Interactive elements: the kind of control, or '' for anything else.
  function interactive(el, tag) {
    const role = (el.getAttribute('role') || '').toLowerCase();
    if (tag === 'a' && el.hasAttribute('href')) return 'link';
    if (tag === 'button' || tag === 'summary') return 'button';
    if (tag === 'select') return 'select';
    if (tag === 'textarea') return 'textbox';
    if (tag === 'input') {
      const type = (el.getAttribute('type') || 'text').toLowerCase();
      if (type === 'hidden') return '';
      if (['submit', 'button', 'reset', 'image'].includes(type)) return 'button';
      if (type === 'checkbox' || type === 'radio') return type;
      if (type === 'password') return 'password';
      return 'textbox';
    }
    if (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) return 'editor';
    if (ROLES.has(role)) return role;
    if (el.hasAttribute('onclick') || (el.tabIndex >= 0 && el.hasAttribute('tabindex') && role !== 'presentation')) {
      // What holds controls of its own, as a strip of tabs does, is no one control: those are.
      return el.querySelector(CONTROLS) ? '' : 'button';
    }
    return '';
  }

  // Whether an element the pointer turns into a hand over is a control a script made of it, as
  // sites built with a framework make menus and lists, whose click handlers are not in the page:
  // one that holds no control of its own and only a few words.
  function pointer(el, tag) {
    if (tag === 'label' || tag === 'body' || tag === 'html' || el.querySelector(CONTROLS)) return false;
    const text = el.innerText.trim();
    return text.length <= 120 && text.split('\n').filter((l) => l.trim()).length <= 3;
  }

  function describe(el, tag, kind, ref) {
    const name = nameOf(el, tag).slice(0, 150);
    let out = '[' + ref + '] ' + kind + (name ? ' "' + name + '"' : ' (' + looks(el) + ')');
    if (kind === 'link') {
      const href = el.href || el.getAttribute('href') || '';
      if (href && !href.startsWith('javascript:')) out += ' → ' + shortUrl(href);
    } else if (kind === 'textbox' || kind === 'searchbox' || kind === 'combobox') {
      const value = 'value' in el ? el.value : el.textContent;
      if (value) out += ' value="' + String(value).slice(0, 200) + '"';
      const type = (el.getAttribute('type') || '').toLowerCase();
      if (type && type !== 'text') out += ' (' + type + ')';
    } else if (kind === 'checkbox' || kind === 'radio' || kind === 'switch') {
      const on = 'checked' in el ? el.checked : el.getAttribute('aria-checked') === 'true';
      out += on ? ' checked' : ' unchecked';
    } else if (kind === 'select') {
      const options = Array.from(el.options || []);
      const chosen = options[el.selectedIndex];
      if (chosen) out += ' = "' + chosen.text.trim() + '"';
      const names = options
        .slice(0, 25)
        .map((o) => o.text.trim())
        .filter(Boolean);
      if (names.length) out += ' options: ' + names.join(' | ') + (options.length > 25 ? ' | …' : '');
    } else if (kind === 'editor') {
      const text = el.innerText.trim();
      if (text) out += ' text="' + text.slice(0, 200) + '"';
    }
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') out += ' (disabled)';
    if (el.getAttribute('aria-expanded') === 'true') out += ' (expanded)';
    // Which tab, sort, filter or page is the one in force: what the assistant checks a click by.
    const aria = (name) => (el.getAttribute(name) || '').toLowerCase();
    if (aria('aria-selected') === 'true') out += ' (selected)';
    if (aria('aria-pressed') === 'true') out += ' (pressed)';
    if (aria('aria-checked') === 'true' && !['checkbox', 'radio', 'switch'].includes(kind)) out += ' (checked)';
    if (aria('aria-current') && aria('aria-current') !== 'false') out += ' (current)';
    return out;
  }

  function nameOf(el, tag) {
    const clean = (s) => (s || '').replace(/\s+/g, ' ').trim();
    const aria = clean(el.getAttribute('aria-label'));
    if (aria) return aria;
    const by = el.getAttribute('aria-labelledby');
    if (by) {
      const text = clean(
        by
          .split(/\s+/)
          .map((id) => (document.getElementById(id) || {}).textContent || '')
          .join(' '),
      );
      if (text) return text;
    }
    if (el.labels && el.labels.length) {
      const text = clean(
        Array.from(el.labels)
          .map((l) => l.innerText)
          .join(' '),
      );
      if (text) return text;
    }
    if (tag === 'input') {
      const type = (el.getAttribute('type') || '').toLowerCase();
      if (['submit', 'button', 'reset'].includes(type) && el.value) return clean(el.value);
      if (type === 'image') return clean(el.getAttribute('alt'));
    }
    const text = tag === 'select' || tag === 'textarea' || tag === 'input' ? '' : clean(el.innerText);
    if (text) return text;
    const img = el.querySelector && el.querySelector('img[alt]');
    if (img && clean(img.getAttribute('alt'))) return clean(img.getAttribute('alt'));
    for (const attr of [
      'placeholder',
      'title',
      'name',
      'data-tooltip',
      'data-title',
      'data-label',
      'aria-description',
    ]) {
      const value = clean(el.getAttribute(attr));
      if (value) return value;
    }
    const svgTitle = el.querySelector && el.querySelector('svg title');
    return svgTitle ? clean(svgTitle.textContent) : '';
  }

  // What tells a control without a name apart: what its page calls it, the icon it shows or the
  // color it is (a color to pick), and where it is on the screen.
  function looks(el) {
    const parts = [];
    const id = ['data-name', 'data-testid', 'data-qa', 'data-action', 'data-icon']
      .map((a) => el.getAttribute(a))
      .find(Boolean);
    const use = el.querySelector('svg use');
    const icon = use && (use.getAttribute('href') || use.getAttribute('xlink:href') || '').split('#')[1];
    if (id) parts.push('marked "' + id.slice(0, 60) + '"');
    else if (icon) parts.push('icon "' + icon.slice(0, 60) + '"');
    const color = swatch(el);
    if (color) parts.push('color ' + color);
    parts.push(placeOf(el));
    return parts.join(', ');
  }

  // The color of a small element that shows only a color, itself or in what it holds, as a hex.
  function swatch(el) {
    const box = el.getBoundingClientRect();
    if (box.width > 64 || box.height > 64 || el.querySelector('svg, img') || el.innerText.trim()) return '';
    for (const part of [el, ...Array.from(el.querySelectorAll('*')).slice(0, 4)]) {
      const m = /rgba?\((\d+), (\d+), (\d+)(?:, ([\d.]+))?\)/.exec(getComputedStyle(part).backgroundColor);
      if (m && (m[4] === undefined || +m[4] > 0)) {
        return (
          '#' +
          m
            .slice(1, 4)
            .map((n) => (+n).toString(16).padStart(2, '0'))
            .join('')
        );
      }
    }
    return '';
  }

  // Where an element is in the view, in words.
  function placeOf(el) {
    const box = el.getBoundingClientRect();
    const x = box.left + box.width / 2;
    const y = box.top + box.height / 2;
    if (y < 0) return 'above the view';
    if (y > window.innerHeight) return 'below the view';
    const row = ['top', 'middle', 'bottom'][Math.min(2, Math.floor((3 * y) / window.innerHeight))];
    const column = ['left', 'center', 'right'][Math.min(2, Math.max(0, Math.floor((3 * x) / window.innerWidth)))];
    return row === 'middle' && column === 'center' ? 'in the middle' : 'at the ' + row + ' ' + column;
  }

  function shortUrl(href) {
    try {
      const u = new URL(href, location.href);
      if (u.origin === location.origin) return u.pathname + u.search + u.hash;
      return u.href;
    } catch (e) {
      return href;
    }
  }

  function shown(el, style) {
    if (el.hidden || el.getAttribute('aria-hidden') === 'true') return false;
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (style.display === 'contents') return true;
    const box = el.getBoundingClientRect();
    if (box.width === 0 && box.height === 0 && style.overflow !== 'visible') return false;
    return true;
  }

  // ---- Acting ----------------------------------------------------------------------------

  // The element a call names: by its reference, or by a CSS selector.
  function find(a) {
    if (a.selector) {
      let el = null;
      try {
        el = document.querySelector(a.selector);
      } catch (e) {
        throw new Error('"' + a.selector + '" is not a valid CSS selector.');
      }
      if (!el) {
        throw new Error(
          'Nothing on the page matches "' +
            a.selector +
            '". Read the page again (browser_read), or try another selector.',
        );
      }
      return el;
    }
    const el = document.querySelector('[' + REF + '="' + CSS.escape(String(a.ref)) + '"]');
    if (!el) {
      throw new Error(
        'There is no [' + a.ref + '] on the page now. Read the page again (browser_read) for fresh references.',
      );
    }
    return el;
  }

  // How a message names the element a call names.
  function label(a) {
    return a.selector ? '"' + a.selector + '"' : '[' + a.ref + ']';
  }

  // An element's reference, given it now if it has none.
  function refFor(el) {
    let r = el.getAttribute(REF);
    if (!r) {
      const next = (window.__demidoRefs || 0) + 1;
      Object.defineProperty(window, '__demidoRefs', {
        value: next,
        writable: true,
        configurable: true,
        enumerable: false,
      });
      r = String(next);
      el.setAttribute(REF, r);
    }
    return r;
  }

  function click(a) {
    const el = find(a);
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') {
      return { error: label(a) + ' is disabled.' };
    }
    if (el.tagName === 'SELECT') {
      return { error: label(a) + ' is a list to choose from: use browser_type with the option to choose.' };
    }
    el.scrollIntoView({ block: 'center', inline: 'center' });
    // A link that would open a new window opens in this tab, where the assistant is.
    const link = el.closest('a[target]');
    if (link) link.setAttribute('target', '_self');
    const box = el.getBoundingClientRect();
    const at = {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      button: 0,
      clientX: box.left + box.width / 2,
      clientY: box.top + box.height / 2,
    };
    el.dispatchEvent(new PointerEvent('pointerdown', at));
    el.dispatchEvent(new MouseEvent('mousedown', at));
    if (typeof el.focus === 'function') el.focus({ preventScroll: true });
    el.dispatchEvent(new PointerEvent('pointerup', at));
    el.dispatchEvent(new MouseEvent('mouseup', at));
    el.click();
    return { ok: true, name: nameOf(el, el.tagName.toLowerCase()).slice(0, 100) };
  }

  // Brings an element into view for a click the app makes at its middle, as the person's mouse
  // would: where that is in the view, and whether the element is what is there (not covered).
  function aim(a) {
    const el = find(a);
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') {
      return { error: label(a) + ' is disabled.' };
    }
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const link = el.closest('a[target]');
    if (link) link.setAttribute('target', '_self');
    const box = el.getBoundingClientRect();
    const x = Math.min(Math.max(box.left + box.width / 2, 0), window.innerWidth - 1);
    const y = Math.min(Math.max(box.top + box.height / 2, 0), window.innerHeight - 1);
    const hit = deepAt(x, y);
    return {
      x,
      y,
      clear: !!hit && (hit === el || el.contains(hit)),
      name: nameOf(el, el.tagName.toLowerCase()).slice(0, 100),
    };
  }

  // The element at a point of the view, inside open shadow roots and frames of the same site.
  // Gives the keyboard to an element, for keys the assistant presses in it next.
  function focus(a) {
    const el = find(a);
    if (el.disabled) return { error: label(a) + ' is disabled.' };
    el.scrollIntoView({ block: 'center', inline: 'center' });
    if (typeof el.focus === 'function') el.focus();
    const owner = keyboardOwner();
    if (owner !== el && !(owner && el.contains(owner))) {
      return { error: label(a) + ' does not take the keyboard: click it with browser_click instead.' };
    }
    if (secret(el)) return { error: label(a) + ' ' + SECRET_FIELD };
    return { ok: true, name: nameOf(el, el.tagName.toLowerCase()).slice(0, 100) };
  }
  function deepAt(x, y) {
    let el = document.elementFromPoint(x, y);
    for (let depth = 0; el && depth < 20; depth++) {
      if (el.shadowRoot) {
        const inner = el.shadowRoot.elementFromPoint(x, y);
        if (!inner || inner === el) break;
        el = inner;
      } else if (el.tagName === 'IFRAME' || el.tagName === 'FRAME') {
        let doc = null;
        try {
          doc = el.contentDocument;
        } catch (e) {
          doc = null;
        }
        if (!doc) break;
        const box = el.getBoundingClientRect();
        x -= box.left + el.clientLeft;
        y -= box.top + el.clientTop;
        const inner = doc.elementFromPoint(x, y);
        if (!inner) break;
        el = inner;
      } else {
        break;
      }
    }
    return el;
  }

  // What is at a point of the view (CSS pixels): the control there, by its reference, or else the
  // element and the start of its text.
  function at(a) {
    const x = +a.x;
    const y = +a.y;
    if (!(x >= 0 && y >= 0 && x < window.innerWidth && y < window.innerHeight)) {
      return { outside: true, width: window.innerWidth, height: window.innerHeight };
    }
    const hit = deepAt(x, y);
    if (!hit) return { kind: '' };
    if (hit.tagName === 'IFRAME' || hit.tagName === 'FRAME') return { kind: 'frame', name: hit.title || '' };
    let el = hit;
    for (let up = hit; up && up.nodeType === Node.ELEMENT_NODE; up = up.parentElement) {
      if (interactive(up, up.tagName.toLowerCase()) || up.hasAttribute(REF)) {
        el = up;
        break;
      }
    }
    const tag = el.tagName.toLowerCase();
    const kind = interactive(el, tag) || (getComputedStyle(el).cursor === 'pointer' ? 'button' : '');
    const text = kind ? nameOf(el, tag) : (el.innerText || '').replace(/\s+/g, ' ').trim();
    return { kind: kind || tag, name: text.slice(0, 80), ref: kind || el.hasAttribute(REF) ? refFor(el) : '' };
  }

  // A click at a point of the view by events a script sends, for where the app cannot click as
  // the person's own mouse does.
  function clickAt(a) {
    const x = +a.x;
    const y = +a.y;
    const el = deepAt(x, y);
    if (!el) return { error: 'There is nothing at ' + Math.round(x) + ', ' + Math.round(y) + ' in the view.' };
    const link = el.closest('a[target]');
    if (link) link.setAttribute('target', '_self');
    const button = a.button === 'right' ? 2 : a.button === 'middle' ? 1 : 0;
    const init = {
      bubbles: true,
      cancelable: true,
      composed: true,
      view: window,
      button,
      buttons: 1 << [0, 2, 1][button],
      clientX: x,
      clientY: y,
      detail: 1,
    };
    const focusable = el.closest(
      'input, textarea, select, button, a[href], [tabindex], [contenteditable=""], [contenteditable="true"]',
    );
    for (let n = 1; n <= (a.double ? 2 : 1); n++) {
      init.detail = n;
      el.dispatchEvent(new PointerEvent('pointerdown', init));
      el.dispatchEvent(new MouseEvent('mousedown', init));
      if (focusable && n === 1) focusable.focus({ preventScroll: true });
      el.dispatchEvent(new PointerEvent('pointerup', init));
      el.dispatchEvent(new MouseEvent('mouseup', init));
      el.dispatchEvent(new MouseEvent(button === 2 ? 'contextmenu' : button === 1 ? 'auxclick' : 'click', init));
    }
    if (a.double && button === 0) el.dispatchEvent(new MouseEvent('dblclick', init));
    return { ok: true };
  }

  // The element that has the keyboard, inside shadow roots and frames of the same site.
  function keyboardOwner() {
    let el = document.activeElement;
    for (let depth = 0; el && depth < 20; depth++) {
      if (el.shadowRoot && el.shadowRoot.activeElement) el = el.shadowRoot.activeElement;
      else if (el.tagName === 'IFRAME' || el.tagName === 'FRAME') {
        let doc = null;
        try {
          doc = el.contentDocument;
        } catch (e) {
          doc = null;
        }
        if (!doc || !doc.activeElement) break;
        el = doc.activeElement;
      } else break;
    }
    if (!el || el.tagName === 'BODY' || el.tagName === 'HTML') return null;
    return el;
  }

  // Whether an element takes typed text.
  function takesText(el) {
    const tag = el.tagName.toLowerCase();
    if (el.isContentEditable || tag === 'textarea') return true;
    if (tag !== 'input') return false;
    const type = (el.getAttribute('type') || 'text').toLowerCase();
    return !['checkbox', 'radio', 'button', 'submit', 'reset', 'image', 'file', 'range', 'color', 'hidden'].includes(
      type,
    );
  }

  // What has the keyboard, for typing where it is.
  function focused() {
    const el = keyboardOwner();
    if (!el) return { none: true };
    const tag = el.tagName.toLowerCase();
    return {
      kind: interactive(el, tag) || tag,
      name: nameOf(el, tag).slice(0, 80),
      ref: refFor(el),
      text: takesText(el) && !el.disabled && !el.readOnly,
      secret: secret(el),
    };
  }

  // Text typed where the keyboard is, by a script, for where the app cannot type as the
  // person's own keyboard does.
  function insert(a) {
    const el = keyboardOwner();
    const text = String(a.text == null ? '' : a.text);
    if (!el || !takesText(el)) {
      return { error: 'Nothing that takes text has the keyboard: click the field first, or pass its reference.' };
    }
    if (secret(el)) return { error: SECRET };
    if (el.ownerDocument.execCommand('insertText', false, text)) return { ok: true };
    if (typeof el.setRangeText === 'function') {
      el.setRangeText(text, el.selectionStart, el.selectionEnd, 'end');
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
      return { ok: true };
    }
    return { error: 'The element with the keyboard does not take text.' };
  }

  // A key pressed where the keyboard is, by events a script sends, with what the browser itself
  // would do for the keys forms use, for where the app cannot press keys as the person's own
  // keyboard does.
  function press(a) {
    const el = keyboardOwner() || document.body;
    const doc = el.ownerDocument;
    const init = {
      key: a.key,
      code: a.code || '',
      keyCode: a.keyCode || 0,
      which: a.keyCode || 0,
      bubbles: true,
      cancelable: true,
      composed: true,
      ctrlKey: !!a.ctrl,
      shiftKey: !!a.shift,
      altKey: !!a.alt,
      metaKey: !!a.meta,
    };
    const typed = a.key.length === 1 && !a.ctrl && !a.alt && !a.meta;
    if (typed && secret(el)) return { error: SECRET };
    let go = el.dispatchEvent(new KeyboardEvent('keydown', init));
    if (go && typed) go = el.dispatchEvent(new KeyboardEvent('keypress', init));
    if (go) {
      if (typed && takesText(el)) {
        doc.execCommand('insertText', false, a.key);
      } else if ((a.ctrl || a.meta) && a.key.toLowerCase() === 'a') {
        if (typeof el.select === 'function') el.select();
        else doc.execCommand('selectAll');
      } else if (a.key === 'Enter') {
        if (el.tagName === 'INPUT' && el.form) {
          if (typeof el.form.requestSubmit === 'function') el.form.requestSubmit();
          else el.form.submit();
        } else if (el.tagName === 'TEXTAREA' || el.isContentEditable) {
          doc.execCommand('insertLineBreak');
        } else if (el.tagName === 'A' || el.tagName === 'BUTTON') {
          el.click();
        }
      } else if (a.key === ' ' && (el.tagName === 'BUTTON' || el.getAttribute('role') === 'button')) {
        el.click();
      } else if (a.key === 'Backspace') {
        doc.execCommand('delete');
      } else if (a.key === 'Delete') {
        doc.execCommand('forwardDelete');
      } else if (a.key === 'Tab') {
        moveFocus(el, a.shift ? -1 : 1);
      }
    }
    el.dispatchEvent(new KeyboardEvent('keyup', init));
    return { ok: true };
  }

  function moveFocus(from, step) {
    const all = Array.from(
      document.querySelectorAll(
        'a[href], button, input:not([type="hidden"]), select, textarea, [tabindex]:not([tabindex^="-"]), [contenteditable=""], [contenteditable="true"]',
      ),
    ).filter((el) => !el.disabled && el.getClientRects().length);
    if (!all.length) return;
    const i = all.indexOf(from);
    const next = i < 0 ? all[step > 0 ? 0 : all.length - 1] : all[(i + step + all.length) % all.length];
    next.focus();
  }

  // The view's size and where it is in the page, in CSS pixels.
  function view() {
    return {
      width: window.innerWidth,
      height: window.innerHeight,
      scrollX: Math.round(window.scrollX),
      scrollY: Math.round(window.scrollY),
      scrollWidth: Math.round(document.documentElement.scrollWidth),
      scrollHeight: Math.round(document.documentElement.scrollHeight),
      dpr: window.devicePixelRatio || 1,
      url: location.href,
      title: document.title,
    };
  }

  // Numbered boxes over the elements in view that the page text numbers, for a screenshot;
  // `a.clear` takes them away again.
  function marks(a) {
    const ID = '__demido_marks';
    const old = document.getElementById(ID);
    if (old) old.remove();
    if (a.clear) return { ok: true };
    const COLORS = ['#d61f45', '#1f7a1f', '#2f55d4', '#c25400', '#8a2bb8', '#00727a', '#7a4f00', '#b0006e'];
    const layer = document.createElement('div');
    layer.id = ID;
    layer.setAttribute('aria-hidden', 'true');
    layer.style.cssText =
      'position:fixed;left:0;top:0;width:100vw;height:100vh;pointer-events:none;z-index:2147483647;';
    const width = window.innerWidth;
    const height = window.innerHeight;
    let marked = 0;
    for (const el of document.querySelectorAll('[' + REF + ']')) {
      if (marked >= 400) break;
      const r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2 || r.bottom <= 0 || r.right <= 0 || r.top >= height || r.left >= width) continue;
      const x = Math.min(Math.max(r.left + r.width / 2, 0), width - 1);
      const y = Math.min(Math.max(r.top + r.height / 2, 0), height - 1);
      const hit = document.elementFromPoint(x, y);
      // Covered by something else, a dialog over the page say.
      if (hit && hit !== el && !el.contains(hit) && !hit.contains(el)) continue;
      const ref = el.getAttribute(REF);
      const color = COLORS[+ref % COLORS.length];
      const box = document.createElement('div');
      box.style.cssText =
        'position:absolute;box-sizing:border-box;border:2px solid ' +
        color +
        ';left:' +
        r.left +
        'px;top:' +
        r.top +
        'px;width:' +
        r.width +
        'px;height:' +
        r.height +
        'px;';
      const tag = document.createElement('span');
      tag.textContent = ref;
      tag.style.cssText =
        'position:absolute;left:-2px;top:' +
        (r.top >= 16 ? '-16px' : '-2px') +
        ';background:' +
        color +
        ';color:#fff;font:bold 11px/14px monospace;padding:1px 3px;border-radius:2px;white-space:nowrap;';
      box.appendChild(tag);
      layer.appendChild(box);
      marked += 1;
    }
    document.documentElement.appendChild(layer);
    return { ok: true, marked };
  }

  // Keeps the page from hearing that it lost the keyboard to the app around it: the app gives
  // the keyboard back to what had it after each click of the assistant's, and a page that heard of
  // it would close the menu or the list of suggestions the click opened. Once per page.
  function keepFocus() {
    if (window.__demidoKeepsFocus) return { ok: true };
    Object.defineProperty(window, '__demidoKeepsFocus', { value: true });
    const real = Document.prototype.hasFocus;
    const quiet = (e) => {
      if (!real.call(document)) e.stopImmediatePropagation();
    };
    window.addEventListener('blur', quiet, true);
    window.addEventListener('focusout', quiet, true);
    document.hasFocus = () => true;
    return { ok: true };
  }

  // ---- The assistant's scripts -----------------------------------------------------------

  // Starts a script of the assistant's, `code`, an async function; `ran` hands over how it ended.
  function run(a, code) {
    if (!window.__demidoRuns) Object.defineProperty(window, '__demidoRuns', { value: {} });
    const runs = window.__demidoRuns;
    runs[a.id] = { done: false };
    Promise.resolve()
      .then(code)
      .then(
        (value) => {
          runs[a.id] = { done: true, value: outcome(value) };
        },
        (e) => {
          runs[a.id] = { done: true, thrown: String(e && e.name && e.message ? e.name + ': ' + e.message : e) };
        },
      );
    return { ok: true };
  }

  // How a script started by `run` ended: `done: false` while it runs, `gone` when the page that
  // ran it is gone.
  // Why a script of the assistant's does not run: the parser's own words, unless the page forbids
  // compiling scripts from text, in which case it says nothing.
  function parse(a) {
    const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
    try {
      new AsyncFunction(a.code);
      return { ok: true };
    } catch (e) {
      return e instanceof SyntaxError ? { error: e.name + ': ' + e.message } : { ok: true };
    }
  }
  function ran(a) {
    const r = window.__demidoRuns && window.__demidoRuns[a.id];
    if (!r) return { gone: true };
    if (!r.done) return { done: false };
    delete window.__demidoRuns[a.id];
    return r;
  }

  // A script's value as text: a string as it is, anything else as JSON, with elements named by
  // their reference so they can be clicked.
  function outcome(value) {
    if (value === undefined) return 'undefined';
    if (typeof value === 'string') return value.slice(0, 100000);
    let budget = 5000;
    const seen = new Set();
    const plain = (v, depth) => {
      if (v === null || typeof v === 'number' || typeof v === 'boolean' || typeof v === 'string') return v;
      if (v === undefined) return null;
      if (typeof v === 'bigint' || typeof v === 'symbol') return String(v);
      if (typeof v === 'function') return '[function ' + (v.name || 'anonymous') + ']';
      if (v === window) return '[window]';
      if (v.nodeType === Node.ELEMENT_NODE) return element(v);
      if (v.nodeType === Node.TEXT_NODE) return '[text "' + v.textContent.trim().slice(0, 200) + '"]';
      if (v.nodeType === Node.DOCUMENT_NODE) return '[document ' + v.URL + ']';
      if (seen.has(v)) return '[circular]';
      if (depth > 8 || --budget < 0) return '[…]';
      if (v instanceof Date) return isNaN(v) ? 'Invalid Date' : v.toISOString();
      if (v instanceof Error || v instanceof RegExp) return String(v);
      seen.add(v);
      let out;
      if (v instanceof Map) {
        out = {};
        for (const [k, x] of Array.from(v).slice(0, 500)) out[String(k)] = plain(x, depth + 1);
      } else if (
        Array.isArray(v) ||
        v instanceof Set ||
        v instanceof NodeList ||
        v instanceof HTMLCollection ||
        ArrayBuffer.isView(v)
      ) {
        const items = Array.from(v);
        out = items.slice(0, 500).map((x) => plain(x, depth + 1));
        if (items.length > 500) out.push('… ' + (items.length - 500) + ' more');
      } else {
        out = {};
        let n = 0;
        for (const k in v) {
          if (++n > 500) {
            out['…'] = 'more keys left out';
            break;
          }
          let x;
          try {
            x = v[k];
          } catch (e) {
            x = '[unreadable]';
          }
          out[k] = plain(x, depth + 1);
        }
      }
      seen.delete(v);
      return out;
    };
    return JSON.stringify(plain(value, 0)).slice(0, 100000);
  }

  // An element as a script's value shows it: its tag, id and classes, reference and name.
  function element(el) {
    const tag = el.tagName.toLowerCase();
    let out = '<' + tag + (el.id ? '#' + el.id : '');
    const classes = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(Boolean) : [];
    if (classes.length) out += '.' + classes.slice(0, 3).join('.');
    out += '> [' + refFor(el) + ']';
    const kind = interactive(el, tag);
    const name = (kind ? nameOf(el, tag) : (el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim()).slice(
      0,
      80,
    );
    return name ? out + ' "' + name + '"' : out;
  }

  function type(a) {
    const el = find(a);
    const tag = el.tagName.toLowerCase();
    const text = String(a.text == null ? '' : a.text);
    if (el.disabled || el.readOnly) return { error: label(a) + ' cannot be typed in.' };
    if (secret(el)) {
      return { error: label(a) + ' ' + SECRET_FIELD };
    }
    el.scrollIntoView({ block: 'center' });
    if (tag === 'select') {
      const want = text.trim().toLowerCase();
      const options = Array.from(el.options);
      const option =
        options.find((o) => o.value === text) ||
        options.find((o) => o.text.trim().toLowerCase() === want) ||
        options.find((o) => o.text.trim().toLowerCase().includes(want));
      if (!option) {
        return {
          error:
            'No option "' +
            text +
            '" in ' +
            label(a) +
            '; its options are: ' +
            options.map((o) => o.text.trim()).join(' | '),
        };
      }
      el.value = option.value;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return { ok: true, chose: option.text.trim() };
    }
    el.focus();
    if (tag === 'input' || tag === 'textarea') {
      const proto = tag === 'input' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
      const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
      // The native setter, so frameworks that track the value (React) see the change.
      setter.call(el, a.append ? el.value + text : text);
      el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
    } else if (el.isContentEditable) {
      if (!a.append) document.execCommand('selectAll', false);
      document.execCommand('insertText', false, text);
    } else {
      return {
        error:
          label(a) +
          ' is not a field to type in. If it takes typing all the same, click it with browser_click, then call ' +
          'browser_type without a reference to type where the keyboard is.',
      };
    }
    if (a.submit) {
      const keys = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
      const down = el.dispatchEvent(new KeyboardEvent('keydown', keys));
      el.dispatchEvent(new KeyboardEvent('keypress', keys));
      el.dispatchEvent(new KeyboardEvent('keyup', keys));
      if (down && el.form) {
        if (typeof el.form.requestSubmit === 'function') el.form.requestSubmit();
        else el.form.submit();
      }
    }
    return { ok: true };
  }

  // Password and payment fields, which the assistant never fills.
  function secret(el) {
    const type = (el.getAttribute('type') || '').toLowerCase();
    const auto = (el.getAttribute('autocomplete') || '').toLowerCase();
    const name = ((el.getAttribute('name') || '') + ' ' + (el.id || '')).toLowerCase();
    return (
      type === 'password' ||
      auto.includes('password') ||
      auto.startsWith('cc-') ||
      /\b(cc-?num|card-?number|cardnumber|cvc|cvv|csc)\b/.test(name)
    );
  }

  function scroll(a) {
    const to = String(a.to || 'down');
    const page = window.innerHeight * 0.85;
    if (to === 'top') window.scrollTo(0, 0);
    else if (to === 'bottom') window.scrollTo(0, document.documentElement.scrollHeight);
    else if (to === 'up') window.scrollBy(0, -page);
    else if (to === 'down') window.scrollBy(0, page);
    else find({ ref: to }).scrollIntoView({ block: 'center' });
    return {
      ok: true,
      scrollY: Math.round(window.scrollY),
      scrollHeight: Math.round(document.documentElement.scrollHeight),
    };
  }
});
