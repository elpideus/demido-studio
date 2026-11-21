/* Conversation: walk the model chip through its states. A model that has just loaded opens its eye. */
(function () {
  'use strict'
  var chip = document.querySelector('[data-chip-demo]')
  var next = document.querySelector('[data-chip-next]')
  var label = document.querySelector('[data-chip-label]')
  if (!chip || !next) return
  var STATES = [
    ['unloaded', 'Unloaded: asleep on disk, uses no memory.'],
    ['queued', 'Waiting for memory: it loads when something unloads.'],
    ['loading', 'Loading: a line travels round the eye until it is ready.'],
    ['loaded', 'Loaded: awake. The eye opens, in the accent colour.'],
    ['generating', 'Generating: awake and working. It breathes.'],
    ['lent', 'Lent: an agent is using it. It comes back.'],
    ['error', 'Failed to load: says why, and what to do.'],
  ]
  var i = 0
  next.addEventListener('click', function () {
    i = (i + 1) % STATES.length
    chip.dataset.state = STATES[i][0]
    if (label) label.textContent = STATES[i][1]
    var glyph = chip.querySelector('.d-glyph')
    glyph.style.animation = 'none'
    void glyph.offsetWidth
    glyph.style.animation = STATES[i][0] === 'loaded' ? 'd-glyph-eye-open var(--d-duration-moderate) var(--d-ease-out) both' : ''
  })
})()
