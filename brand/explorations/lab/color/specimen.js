// Demido Studio colour specimen: fills the swatches from out/meta.js and draws the chart.
(function () {
  var root = document.documentElement, M = window.DEMIDO_META;
  var theme = root.dataset.theme, neutral = root.dataset.neutral, accent = root.dataset.accent, primary = root.dataset.primary;
  var el = function (tag, cls, html) { var e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
  var $ = function (id) { return document.getElementById(id); };

  // ---------- neutral ramp ----------
  var N = M.neutrals[neutral][theme], A = M.accents[accent], Ath = A[theme], S = M.semantics[accent][theme];
  $('cap-neutral').textContent = M.temps[neutral] + ', ' + theme;
  var lum = function (hex) { var c = [1, 3, 5].map(function (i) { var v = parseInt(hex.slice(i, i + 2), 16) / 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }); return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]; };
  var ink = function (hex) { return lum(hex) > 0.36 ? '#0e0e0e' : '#ffffff'; };
  M.steps[theme].forEach(function (s, i) {
    var c = N[s.k], d = el('div', '', '<b>' + (i + 1) + '</b>' + s.k + '<br><span class="sw-hex">' + c.hex.slice(1) + '</span>');
    d.style.background = c.hex; d.style.color = ink(c.hex); d.title = s.role + ' ' + c.oklch;
    $('ramp').appendChild(d);
  });

  // ---------- accent swatches ----------
  $('cap-accent').textContent = A.label + ' · ' + Ath.base.oklch;
  [['Base', Ath.base.hex], ['Hover', Ath.hover.hex], ['Pressed', Ath.pressed.hex], ['Tint', Ath.tint.hex], ['As text', Ath.text.hex], ['Text on fill', Ath.base.hex, Ath.onAccent]].forEach(function (r) {
    var w = el('div', 'sw'), i = el('i', r[2] ? 'on' : '');
    i.style.background = r[1];
    if (r[0] === 'As text') { i.style.background = N.raised.hex; i.className = 'on'; i.style.color = r[1]; i.textContent = 'Loaded'; }
    if (r[2]) { i.style.color = r[2]; i.textContent = 'Aa'; }
    w.appendChild(i); w.appendChild(el('span', '', r[0])); w.appendChild(el('span', 'sw-hex', r[2] ? r[2] : r[1]));
    $('sw-accent').appendChild(w);
  });

  // ---------- signals ----------
  [['success', 'Success', 'i-task-done'], ['caution', 'Caution', 'i-caution'], ['danger', 'Danger', 'i-danger'], ['info', 'Info', 'i-info']].forEach(function (r) {
    var c = S[r[0]], w = el('div', 'sw sem'), i = el('i', '', '<svg class="i"><use href="#' + r[2] + '"/></svg>' + r[1]);
    i.style.background = c.tint.hex; i.style.color = c.fg.hex; i.style.boxShadow = 'inset 0 0 0 1px ' + c.line.hex;
    w.appendChild(i); w.appendChild(el('span', 'sw-hex', c.fg.hex + ' on ' + c.tint.hex + (c.shifted ? ' *' : '')));
    $('sw-sem').appendChild(w);
  });

  $('stamp').innerHTML = '<span>neutral=' + neutral + ' accent=' + accent + ' theme=' + theme + ' primary=' + primary + '</span><span>Demido Studio colour lab</span>';

  // ---------- chart: tokens per second, four models, last 60 s ----------
  var names = ['qwen2.5-coder 7B', 'phi-4-mini', 'llama3.2 3B', 'gemma2 9B'];
  var levels = [38, 54, 67, 21], amp = [3.2, 4.2, 3.6, 2.2];
  var seed = 7; var rnd = function () { seed = (seed * 16807) % 2147483647; return seed / 2147483647; };
  var data = levels.map(function (lv, k) {
    var out = [], v = lv;
    for (var t = 0; t <= 60; t += 2) { v += (rnd() - 0.5) * amp[k] * 1.6 + (lv - v) * 0.35; out.push(Math.round(v * 10) / 10); }
    return out;
  });
  var W = 624, H = 150, L = 30, R = 44, T = 8, B = 22, ymax = 80;
  var x = function (i) { return L + (i / 30) * (W - L - R); }, y = function (v) { return T + (1 - v / ymax) * (H - T - B); };
  var NS = 'http://www.w3.org/2000/svg', svg = $('chart');
  var mk = function (tag, attrs, text) { var e = document.createElementNS(NS, tag); for (var k in attrs) e.setAttribute(k, attrs[k]); if (text != null) e.textContent = text; svg.appendChild(e); return e; };
  [0, 40, 80].forEach(function (v) { mk('line', { class: 'grid', x1: L, x2: W - R, y1: y(v), y2: y(v) }); mk('text', { class: 'axis', x: L - 8, y: y(v) + 3.5, 'text-anchor': 'end' }, v); });
  [[0, '-60 s'], [15, '-30 s'], [30, 'now']].forEach(function (p) { mk('text', { class: 'axis', x: x(p[0]), y: H - 5, 'text-anchor': p[0] === 0 ? 'start' : p[0] === 30 ? 'end' : 'middle' }, p[1]); });
  var cross = mk('line', { class: 'cross', y1: T, y2: H - B, x1: -9, x2: -9 });
  data.forEach(function (d, k) {
    var col = 'var(--s' + (k + 1) + ')';
    mk('path', { class: 'line', d: d.map(function (v, i) { return (i ? 'L' : 'M') + x(i).toFixed(1) + ' ' + y(v).toFixed(1); }).join(' '), style: 'stroke:' + col + (k === 0 ? ';stroke-width:2.5' : '') });
    mk('circle', { class: 'end', cx: x(30), cy: y(d[30]), r: 4, style: 'fill:' + col });
    mk('text', { class: 'lab', x: x(30) + 9, y: y(d[30]) + 4 }, d[30].toFixed(0));
    var li = el('span', '', '<i style="background:' + col + '"></i>' + names[k]);
    $('legend').appendChild(li);
  });
  // hover: crosshair and one tooltip for all four series
  var tip = $('tip');
  svg.addEventListener('mousemove', function (ev) {
    var b = svg.getBoundingClientRect(), i = Math.max(0, Math.min(30, Math.round(((ev.clientX - b.left) / b.width * W - L) / (W - L - R) * 30)));
    cross.setAttribute('x1', x(i)); cross.setAttribute('x2', x(i));
    tip.hidden = false;
    tip.innerHTML = '<div style="color:var(--n-text3)">' + (i === 30 ? 'now' : '-' + (60 - i * 2) + ' s') + '</div>' + data.map(function (d, k) { return '<div><i style="background:var(--s' + (k + 1) + ')"></i>' + names[k] + '<b>' + d[i].toFixed(1) + '</b></div>'; }).join('');
    tip.style.left = Math.min(x(i) + 26, W - 170) + 'px'; tip.style.top = '44px';
  });
  svg.addEventListener('mouseleave', function () { tip.hidden = true; cross.setAttribute('x1', -9); cross.setAttribute('x2', -9); });
})();
