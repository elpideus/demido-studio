// What Demido reads of a page in its browser (DECISIONS Z5). Run in a world of Demido's own, apart from the
// page's scripts, so a page cannot change what this sees by changing the browser's own functions. It numbers
// what can be pressed or filled, at most 40, as a person would see them (shown, not disabled), and gives back
// the page's HTML as it is after its scripts ran.
(() => {
  const MOST = 40;
  const items = [];
  const shown = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 1 && r.height > 1 && s.visibility !== 'hidden' && s.display !== 'none' && Number(s.opacity) > 0.05;
  };
  const flat = (t) => (t || '').replace(/\s+/g, ' ').trim().slice(0, 80);
  const typed = ['text', 'search', 'email', 'url', 'tel', 'number', 'password', ''];
  const isTextField = (el) => el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && typed.includes((el.getAttribute('type') || '').toLowerCase())) || el.isContentEditable;
  for (const el of document.querySelectorAll('[data-demido-n]')) el.removeAttribute('data-demido-n');
  const all = document.querySelectorAll('a[href], button, input, textarea, select, [role=button], [role=link], [role=searchbox], [role=textbox], [contenteditable=true]');
  for (const el of all) {
    if (items.length >= MOST) break;
    const tag = el.tagName.toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (tag === 'input' && type === 'hidden') continue;
    if (el.disabled || !shown(el)) continue;
    let kind;
    const role = el.getAttribute('role');
    if (tag === 'a' || role === 'link') kind = 'link';
    else if (tag === 'button' || role === 'button' || (tag === 'input' && ['submit', 'button', 'reset', 'image'].includes(type))) kind = 'button';
    else if (tag === 'input' && (type === 'checkbox' || type === 'radio')) kind = type;
    else if (tag === 'select') kind = 'choice';
    else if (isTextField(el) || role === 'searchbox' || role === 'textbox') kind = 'field';
    else continue;
    let words = flat(el.getAttribute('aria-label') || (kind === 'field' ? '' : el.innerText) || (kind === 'button' ? el.value : '') || el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt'));
    if (!words && el.labels && el.labels[0]) words = flat(el.labels[0].innerText);
    if (!words && kind === 'link') { const img = el.querySelector('img[alt]'); if (img) words = flat(img.alt); }
    if (!words) words = flat(el.getAttribute('name'));
    const n = items.length + 1;
    el.setAttribute('data-demido-n', String(n));
    let alone = false;
    if (kind === 'field' && el.form) {
      alone = [...el.form.elements].filter((f) => isTextField(f) && f.type !== 'hidden' && shown(f)).length === 1;
    }
    items.push({
      n, kind, words,
      href: kind === 'link' && el.href ? String(el.href) : null,
      password: tag === 'input' && type === 'password',
      alone,
      choices: kind === 'choice' ? [...el.options].slice(0, 12).map((o) => flat(o.text)) : [],
    });
  }
  return { title: document.title || '', address: String(location.href), html: document.documentElement.outerHTML.slice(0, 2000000), items };
})()
