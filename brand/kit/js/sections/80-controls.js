/* Controls: the permission-level description follows the choice; sliders show their value and fill. */
(function () {
  'use strict'
  var PERM = {
    cautious: 'Always asks before acting. Very safe, and it interrupts often.',
    balanced: 'Asks only before things that could cause damage.',
    autonomous: 'Never asks. Use it only for work you can undo.',
    smart: 'The model decides when to ask. Every decision is logged.',
  }
  var group = document.querySelector('[data-perm-demo]')
  var text = document.querySelector('[data-perm-text]')
  if (group && text) {
    group.addEventListener('change', function (e) {
      text.textContent = PERM[e.target.value] || ''
    })
  }

  document.querySelectorAll('input[type="range"][data-slider]').forEach(function (input) {
    var out = input.parentElement.querySelector('output')
    var decimals = input.dataset.decimals === undefined ? 2 : Number(input.dataset.decimals)
    var suffix = input.dataset.suffix || ''
    function sync() {
      var min = Number(input.min || 0)
      var max = Number(input.max || 100)
      var v = Number(input.value)
      input.style.setProperty('--d-slider-fill', ((v - min) / (max - min)) * 100 + '%')
      if (out) out.textContent = v.toFixed(decimals) + suffix
    }
    input.addEventListener('input', sync)
    sync()
  })
})()
