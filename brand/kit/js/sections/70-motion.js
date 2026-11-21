/* Motion: token players, the interruptible spring reference, and replayable signature moves. */
(function () {
  'use strict'

  // ---------- token players ----------
  var players = document.querySelector('[data-motion-players]')
  var play = document.querySelector('[data-motion-play]')
  if (players && play) {
    play.addEventListener('click', function () {
      if (players.hasAttribute('data-on')) players.removeAttribute('data-on')
      else players.setAttribute('data-on', '')
      play.textContent = players.hasAttribute('data-on') ? 'Play back' : 'Play'
    })
  }

  // ---------- signature replays ----------
  var KEYFRAMES = {
    eye: 'd-glyph-eye-open var(--d-duration-moderate) var(--d-ease-out) both',
    done: 'd-glyph-settle var(--d-spring-snappy-duration) var(--d-spring-snappy) both',
  }
  document.querySelectorAll('[data-sig-play]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var el = document.querySelector('[data-sig="' + btn.dataset.sigPlay + '"]')
      if (!el) return
      el.style.animation = 'none'
      void el.offsetWidth
      el.style.animation = KEYFRAMES[btn.dataset.sigPlay]
    })
  })

  // ---------- the interruptible spring ----------
  // Reference implementation. One spring on x; state is position and velocity, so a new target or a new
  // grab always continues from the live values. Parameters are Apple-style: response (s) and damping ratio.
  var stage = document.querySelector('[data-spring-demo]')
  if (!stage) return
  var card = stage.querySelector('.kit-motion__card')
  var x = 0 // current position, px
  var v = 0 // current velocity, px/s
  var target = 0
  var raf = 0
  var last = 0
  var damping = 1
  var RESPONSE = 0.3
  var reduce = window.matchMedia('(prefers-reduced-motion: reduce)')
  var stops = [0, 0, 0]
  var index = 0

  function measure() {
    var pad = 48
    var max = stage.clientWidth - card.offsetWidth - pad
    stops = [pad, (pad + max) / 2, max]
  }
  function render() {
    card.style.transform = 'translate3d(' + x.toFixed(2) + 'px,0,0)'
  }
  function settle(i) {
    index = i
    card.setAttribute('aria-valuenow', String(i + 1))
    card.setAttribute('aria-valuetext', 'Stop ' + (i + 1) + ' of 3')
  }
  function tick(now) {
    var dt = Math.min(0.032, (now - last) / 1000)
    last = now
    var w = (2 * Math.PI) / RESPONSE
    // semi-implicit Euler in small steps: stable for these stiffness values
    var steps = 4
    for (var s = 0; s < steps; s++) {
      var h = dt / steps
      var a = -w * w * (x - target) - 2 * damping * w * v
      v += a * h
      x += v * h
    }
    render()
    if (Math.abs(x - target) < 0.1 && Math.abs(v) < 2) {
      x = target
      v = 0
      render()
      raf = 0
      return
    }
    raf = requestAnimationFrame(tick)
  }
  function springTo(t, velocity, bounce) {
    target = t
    v = velocity
    damping = bounce ? 0.8 : 1
    if (reduce.matches) {
      x = target
      v = 0
      render()
      return
    }
    if (!raf) {
      last = performance.now()
      raf = requestAnimationFrame(tick)
    }
  }
  // Apple's projection: where would this speed carry the card?
  function project(velocity) {
    var d = 0.998
    return ((velocity / 1000) * d) / (1 - d)
  }
  // Past the ends the card follows less and less.
  function rubber(over, dim) {
    var c = 0.55
    return (over * dim * c) / (dim + c * Math.abs(over))
  }

  var drag = null
  card.addEventListener('pointerdown', function (e) {
    if (e.button !== 0 && e.pointerType === 'mouse') return
    card.setPointerCapture(e.pointerId)
    cancelAnimationFrame(raf)
    raf = 0
    // grab offset: the card stays under the same point of the pointer; x is the LIVE position, mid-flight or not
    drag = { id: e.pointerId, offset: e.clientX - x, samples: [{ t: e.timeStamp, x: e.clientX }] }
    card.setAttribute('data-dragging', '')
  })
  card.addEventListener('pointermove', function (e) {
    if (!drag || e.pointerId !== drag.id) return
    var raw = e.clientX - drag.offset
    var min = stops[0]
    var max = stops[2]
    if (raw < min) raw = min + rubber(raw - min, stage.clientWidth)
    else if (raw > max) raw = max + rubber(raw - max, stage.clientWidth)
    x = raw
    render()
    drag.samples.push({ t: e.timeStamp, x: e.clientX })
    if (drag.samples.length > 6) drag.samples.shift()
  })
  function release(e) {
    if (!drag || e.pointerId !== drag.id) return
    var s = drag.samples
    var first = s[0]
    var lastS = s[s.length - 1]
    var dt = (lastS.t - first.t) / 1000
    var velocity = dt > 0.008 && e.timeStamp - lastS.t < 80 ? (lastS.x - first.x) / dt : 0
    drag = null
    card.removeAttribute('data-dragging')
    var landing = x + project(velocity)
    var best = 0
    stops.forEach(function (stop, i) {
      if (Math.abs(stop - landing) < Math.abs(stops[best] - landing)) best = i
    })
    settle(best)
    springTo(stops[best], velocity, Math.abs(velocity) > 600) // a little bounce only after a real flick
  }
  card.addEventListener('pointerup', release)
  card.addEventListener('pointercancel', release)

  // Keyboard: no animation, by rule.
  card.addEventListener('keydown', function (e) {
    var next = index
    if (e.key === 'ArrowRight' || e.key === 'ArrowUp') next = Math.min(2, index + 1)
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') next = Math.max(0, index - 1)
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = 2
    else return
    e.preventDefault()
    cancelAnimationFrame(raf)
    raf = 0
    settle(next)
    x = target = stops[next]
    v = 0
    render()
  })

  function init() {
    measure()
    x = target = stops[index]
    v = 0
    render()
  }
  init()
  window.addEventListener('resize', init)
})()
