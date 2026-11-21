/* Principles: two small live specimens. */
(function () {
  'use strict'
  // 1. Quiet until it matters: colour appears only on the rows that need attention.
  var quiet = document.querySelector('[data-quiet-demo]')
  if (quiet) {
    var LABEL = { done: 'Done', 'needs-input': 'Needs you', failed: 'Failed' }
    quiet.addEventListener('change', function (e) {
      var attention = e.target.value === 'attention'
      quiet.querySelectorAll('.d-task').forEach(function (task) {
        var state = attention && task.dataset.whenAttention ? task.dataset.whenAttention : 'done'
        task.dataset.state = state
        var glyph = task.querySelector('.d-glyph')
        glyph.setAttribute('aria-label', LABEL[state])
        var label = task.querySelector('.d-task__label')
        if (label.dataset.normal) label.textContent = attention ? label.dataset.attention : label.dataset.normal
      })
    })
  }

  // 5. Motion explains: a model loads (the eye turns), then wakes (the eye opens).
  var chip = document.querySelector('[data-wake-demo]')
  var btn = document.querySelector('[data-wake-btn]')
  if (chip && btn) {
    var timer
    btn.addEventListener('click', function () {
      var glyph = chip.querySelector('.d-glyph')
      clearTimeout(timer)
      glyph.style.animation = ''
      if (chip.dataset.state !== 'unloaded') {
        chip.dataset.state = 'unloaded'
        btn.textContent = 'Load'
        return
      }
      chip.dataset.state = 'loading'
      btn.textContent = 'Loading'
      timer = setTimeout(function () {
        chip.dataset.state = 'loaded'
        glyph.style.animation = 'd-glyph-eye-open var(--d-duration-moderate) var(--d-ease-out) both'
        btn.textContent = 'Unload'
      }, 1400)
    })
  }
})()
