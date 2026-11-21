/* State glyphs: a small task-list demo that steps through states the way a session does. */
(function () {
  'use strict'
  var list = document.querySelector('[data-tasks-demo]')
  if (!list) return
  var LABEL = { pending: 'Pending', running: 'In progress', done: 'Done', delegated: 'With an agent' }
  var initial = Array.prototype.map.call(list.querySelectorAll('.d-task'), function (t) {
    return t.dataset.state
  })

  function set(task, state) {
    task.dataset.state = state
    var glyph = task.querySelector(':scope > .d-task__row > .d-glyph')
    glyph.setAttribute('aria-label', LABEL[state] || state)
    if (state !== 'running') {
      glyph.removeAttribute('data-settle')
      void glyph.offsetWidth
      glyph.setAttribute('data-settle', '')
    }
  }

  // Leaves first, then parents: a parent is done only when all its sub-tasks are.
  function step() {
    var tasks = Array.prototype.slice.call(list.querySelectorAll('.d-task'))
    var open = tasks.filter(function (t) {
      return t.dataset.state !== 'done'
    })
    if (!open.length) return reset()
    var leaf = open.find(function (t) {
      return !t.querySelector('.d-task:not([data-state="done"])') && (t.dataset.state === 'running' || t.dataset.state === 'delegated')
    })
    if (leaf) {
      set(leaf, 'done')
    }
    var next = tasks.find(function (t) {
      return t.dataset.state === 'pending'
    })
    var active = tasks.some(function (t) {
      return t.dataset.state === 'running' && !t.querySelector('.d-task')
    })
    if (next && !active && !leaf) set(next, 'running')
    else if (next && leaf && !tasks.some(function (t) { return t.dataset.state === 'delegated' })) set(next, 'running')
    // parents follow their children
    tasks.forEach(function (t) {
      var kids = t.querySelectorAll(':scope > .d-tasks > .d-task')
      if (!kids.length) return
      var allDone = Array.prototype.every.call(kids, function (k) { return k.dataset.state === 'done' })
      var anyStarted = Array.prototype.some.call(kids, function (k) { return k.dataset.state !== 'pending' })
      if (allDone && t.dataset.state !== 'done') set(t, 'done')
      else if (!allDone && anyStarted && t.dataset.state === 'pending') set(t, 'running')
    })
  }

  function reset() {
    Array.prototype.forEach.call(list.querySelectorAll('.d-task'), function (t, i) {
      t.dataset.state = initial[i]
      t.querySelector(':scope > .d-task__row > .d-glyph').removeAttribute('data-settle')
    })
  }

  var stepBtn = document.querySelector('[data-tasks-step]')
  var resetBtn = document.querySelector('[data-tasks-reset]')
  if (stepBtn) stepBtn.addEventListener('click', step)
  if (resetBtn) resetBtn.addEventListener('click', reset)
})()
