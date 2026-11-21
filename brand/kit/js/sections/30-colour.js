/* Colour: the Accent setting (as the app will have it), swatch copying, and live contrast.
   The page sets one custom property, --d-color-accent-source, on <html>. Every accent token derives from it. */
(function () {
  'use strict'
  var root = document.documentElement
  var KEY = 'demido-kit-accent'
  var GREEN = '#68efaf'

  // ---------- helpers ----------
  var canvas = document.createElement('canvas')
  canvas.width = canvas.height = 1
  var ctx = canvas.getContext('2d', { willReadFrequently: true })
  function rgb(color) {
    ctx.clearRect(0, 0, 1, 1)
    ctx.fillStyle = '#000'
    ctx.fillStyle = color
    ctx.fillRect(0, 0, 1, 1)
    var d = ctx.getImageData(0, 0, 1, 1).data
    return [d[0], d[1], d[2]]
  }
  function hex(c) {
    return '#' + c.map(function (v) { return v.toString(16).padStart(2, '0') }).join('')
  }
  function lum(c) {
    var v = c.map(function (x) {
      x /= 255
      return x <= 0.04045 ? x / 12.92 : Math.pow((x + 0.055) / 1.055, 2.4)
    })
    return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
  }

  // ---------- the Accent setting ----------
  var state = { mode: 'auto', custom: GREEN, sim: '' }
  try {
    var saved = JSON.parse(localStorage.getItem(KEY) || 'null')
    if (saved && typeof saved === 'object') {
      if (['auto', 'green', 'custom'].indexOf(saved.mode) > -1) state.mode = saved.mode
      if (/^#[0-9a-f]{6}$/i.test(saved.custom || '')) state.custom = saved.custom
    }
    var q = new URLSearchParams(location.search)
    if (['auto', 'green', 'custom'].indexOf(q.get('accent')) > -1) state.mode = q.get('accent')
    if (q.get('sim')) state.sim = '#' + q.get('sim')
    if (q.get('custom')) state.custom = '#' + q.get('custom')
  } catch (e) {}

  var modeSel = document.querySelector('[data-accent-mode]')
  var customIn = document.querySelector('[data-accent-custom]')
  var desc = document.querySelector('[data-accent-desc]')
  var sourceText = document.querySelector('[data-accent-source-text]')
  var changed = document.querySelector('[data-accent-changed]')
  var simGroup = document.querySelector('[data-accent-sim]')

  var DESC = {
    auto: "Uses your system's accent colour when it has one. Otherwise Demido green.",
    green: 'Always Demido green.',
    custom: 'A colour you choose.',
  }

  // Which colour is the accent, and why. This is the rule the app implements.
  function resolve() {
    if (state.mode === 'custom') return { color: state.custom, why: 'Your colour, ' + state.custom + '.' }
    if (state.mode === 'auto') {
      if (state.sim) return { color: state.sim, why: 'The system accent, ' + state.sim + '.' }
      return { color: null, why: 'The system reports no accent, so Demido green.' }
    }
    return { color: null, why: 'Demido green.' }
  }

  function apply() {
    var r = resolve()
    if (r.color) root.style.setProperty('--d-color-accent-source', r.color)
    else root.style.removeProperty('--d-color-accent-source')
    if (modeSel) modeSel.value = state.mode
    if (customIn) {
      customIn.hidden = state.mode !== 'custom'
      customIn.value = state.custom
    }
    if (desc) desc.textContent = DESC[state.mode]
    if (sourceText) sourceText.textContent = r.why
    if (changed) changed.hidden = state.mode === 'auto'
    if (simGroup) {
      simGroup.querySelectorAll('button').forEach(function (b) {
        b.setAttribute('aria-pressed', String((b.dataset.sim || '') === state.sim))
      })
    }
    document.querySelectorAll('[data-accent-hex]').forEach(function (out) {
      var scope = out.closest('[data-theme]') || root
      out.textContent = hex(rgb(getComputedStyle(scope).getPropertyValue('--d-color-accent-base').trim()))
    })
    measure()
    try {
      localStorage.setItem(KEY, JSON.stringify({ mode: state.mode, custom: state.custom }))
    } catch (e) {}
  }

  if (modeSel) {
    modeSel.addEventListener('change', function () {
      state.mode = modeSel.value
      apply()
    })
  }
  if (customIn) {
    customIn.addEventListener('input', function () {
      state.custom = customIn.value
      apply()
    })
  }
  if (simGroup) {
    simGroup.addEventListener('click', function (e) {
      var b = e.target.closest('button')
      if (!b) return
      state.sim = b.dataset.sim || ''
      apply()
    })
  }

  // ---------- swatches copy their token ----------
  document.addEventListener('click', function (e) {
    var sw = e.target.closest('.kit-colour__sw')
    if (!sw) return
    var token = sw.dataset.token
    var done = function () {
      if (window.DemidoKit) window.DemidoKit.say('Copied ' + token)
    }
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(token).then(done, done)
    else done()
  })

  // ---------- live contrast (WCAG 2) ----------
  function measure() {
    document.querySelectorAll('[data-contrast]').forEach(function (out) {
      var scope = out.closest('[data-theme]') || root
      var cs = getComputedStyle(scope)
      var fg = lum(rgb(cs.getPropertyValue(out.dataset.contrast).trim()))
      var bg = lum(rgb(cs.getPropertyValue('--d-color-bg-canvas').trim()))
      var ratio = (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05)
      out.textContent = ratio.toFixed(1) + ':1'
    })
  }

  apply()
  document.addEventListener('kit:theme', apply)
})()
