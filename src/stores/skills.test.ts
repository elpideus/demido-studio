import { describe, it, expect } from 'vitest'
import { commandsOf, expandCommand, skillsContext, usageOf, withSkillLocation, type SkillEntry } from './skills'

const skill = (over: Partial<SkillEntry>): SkillEntry => ({
  id: 'a',
  name: 'A',
  description: '',
  version: '1.0.0',
  commands: [],
  tools: [],
  metaJson: '{"id":"a"}',
  files: [],
  path: 'C:\\skills\\a',
  enabled: true,
  ...over,
})

describe('skillsContext', () => {
  const s = skill({
    name: 'Alpha',
    metaJson: '{"id":"a","name":"Alpha"}',
    files: ['C:\\skills\\a\\SKILL.md', 'C:\\skills\\a\\refs\\x.md'],
  })

  it('inlines skill.json and lists the file paths', () => {
    const ctx = skillsContext([s])
    expect(ctx).toContain('{"id":"a","name":"Alpha"}')
    expect(ctx).toContain('C:\\skills\\a\\SKILL.md')
    expect(ctx).toContain('C:\\skills\\a\\refs\\x.md')
  })

  it('excludes disabled skills', () => {
    expect(skillsContext([{ ...s, enabled: false }])).toBe('')
  })
})

describe('commandsOf', () => {
  it('only surfaces commands from enabled skills', () => {
    const cmds = commandsOf([
      skill({ id: 'on', commands: [{ name: 'go', description: '' }] }),
      skill({ id: 'off', enabled: false, commands: [{ name: 'hidden', description: '' }] }),
    ])
    expect(cmds.map(c => c.invocation)).toEqual(['go'])
  })

  it('qualifies colliding names with the skill id, leaving unique ones bare', () => {
    const cmds = commandsOf([
      skill({ id: 'one', commands: [{ name: 'dup', description: '' }, { name: 'solo', description: '' }] }),
      skill({ id: 'two', commands: [{ name: 'dup', description: '' }] }),
    ])
    expect(cmds.map(c => c.invocation).sort()).toEqual(['one:dup', 'solo', 'two:dup'])
  })

  it('does not let a disabled skill trigger qualification of a unique name', () => {
    const cmds = commandsOf([
      skill({ id: 'one', commands: [{ name: 'dup', description: '' }] }),
      skill({ id: 'two', enabled: false, commands: [{ name: 'dup', description: '' }] }),
    ])
    expect(cmds.map(c => c.invocation)).toEqual(['dup'])
  })

  it('carries the source skill through for lookup and display', () => {
    const [cmd] = commandsOf([
      skill({ id: 'sk', name: 'Skill', commands: [{ name: 'go', description: 'd', file: 'p.md' }] }),
    ])
    expect(cmd).toMatchObject({ skillId: 'sk', skillName: 'Skill', file: 'p.md' })
  })
})

describe('expandCommand', () => {
  it('substitutes every $ARGUMENTS occurrence', () => {
    expect(expandCommand('do $ARGUMENTS twice: $ARGUMENTS', 'x')).toBe('do x twice: x')
  })

  it('appends args when the body has no placeholder', () => {
    expect(expandCommand('Review the code.\n', 'src/main.rs')).toBe('Review the code.\n\nsrc/main.rs')
  })

  it('leaves the body untouched when there are no args', () => {
    expect(expandCommand('Just do it.', '')).toBe('Just do it.')
  })

  it('blanks the placeholder when invoked without args', () => {
    expect(expandCommand('context: $ARGUMENTS', '')).toBe('context: ')
  })

  it('leaves an escaped placeholder literal instead of substituting prose that discusses it', () => {
    // The bug this guards: a guide saying "rewrite $1 to $ARGUMENTS" had its own instruction
    // replaced with the user's argument.
    const body = 'Rewrite positional args to \\$ARGUMENTS. Target: $ARGUMENTS'
    expect(expandCommand(body, 'C:\\skills\\caveman')).toBe(
      'Rewrite positional args to $ARGUMENTS. Target: C:\\skills\\caveman',
    )
  })

  it('treats a body with only escaped placeholders as having none, so args still arrive', () => {
    expect(expandCommand('Use \\$ARGUMENTS in your command.', 'x')).toBe(
      'Use $ARGUMENTS in your command.\n\nx',
    )
  })

  it('substitutes positional placeholders', () => {
    expect(expandCommand('move $1 to $2', 'a.txt b.txt')).toBe('move a.txt to b.txt')
  })

  it('keeps a quoted token whole', () => {
    expect(expandCommand('open $1 | rest $2', '"two words" tail')).toBe('open two words | rest tail')
  })

  it('blanks a positional that was not supplied', () => {
    expect(expandCommand('$1/$2', 'only')).toBe('only/')
  })

  it('binds declared params by name in schema order', () => {
    const out = expandCommand('port $file to $lang', 'a.rs rust', [
      { name: 'file' }, { name: 'lang' },
    ])
    expect(out).toBe('port a.rs to rust')
  })

  it('lets a rest param swallow the remaining tokens', () => {
    const out = expandCommand('[$level] $text', 'high fix the parser now', [
      { name: 'level' }, { name: 'text', rest: true },
    ])
    expect(out).toBe('[high] fix the parser now')
  })

  it('throws when a required param is missing rather than sending a half-filled prompt', () => {
    expect(() => expandCommand('do $file', '', [{ name: 'file', required: true }]))
      .toThrow(/missing required argument: file/)
  })

  it('leaves an undeclared $word alone — prose is not a placeholder', () => {
    expect(expandCommand('cost is $total for $1', 'x', [])).toBe('cost is $total for x')
  })

  it('honours the escape on positional placeholders too', () => {
    expect(expandCommand('write \\$1 in the body. arg: $1', 'v')).toBe('write $1 in the body. arg: v')
  })
})

describe('usageOf', () => {
  it('renders required, optional and rest params distinctly', () => {
    const usage = usageOf({
      invocation: 'port', name: 'port',
      params: [{ name: 'file', required: true }, { name: 'lang' }, { name: 'notes', rest: true }],
    })
    expect(usage).toBe('/port <file> [lang] [notes...]')
  })

  it('renders a bare command when there are no params', () => {
    expect(usageOf({ invocation: 'go', name: 'go' })).toBe('/go')
  })
})

describe('withSkillLocation', () => {
  it('prefixes the install path so relative paths in the body resolve', () => {
    const out = withSkillLocation('Read commands/go.md', 'Cmd', 'C:\\skills\\cmd')
    expect(out).toContain('C:\\skills\\cmd')
    expect(out.endsWith('Read commands/go.md')).toBe(true)
  })

  it('passes the body through untouched when no path is known', () => {
    expect(withSkillLocation('body', 'N', '')).toBe('body')
  })
})
