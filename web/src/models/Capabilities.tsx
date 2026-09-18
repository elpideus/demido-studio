import { AudioLines, Brain, Eye, Wrench, type LucideIcon } from 'lucide-react'

import type { Capabilities as Facts, Fact } from '@/setup/setup'
import styles from './Capabilities.module.css'

/**
 * A model's capabilities as tags (`design/shell.md`), wherever a model
 * appears: a row, the detail pane, the composer.
 *
 * **Three states, not two** ([#75](https://github.com/elpideus/demido-studio/issues/75)).
 * `yes` is drawn in its `--cap-*` colour. `no` is greyed and never hidden,
 * which only a file on disk can say. `unknown` is what a repository in the
 * index has for everything its publisher did not state, and it is drawn
 * visibly apart from `no`, unfilled and with a question mark, because
 * a tag claiming an absence nobody measured would be a guess dressed as a
 * fact.
 *
 * `from` says whose statement it is, which is what the tooltip tells a person
 * who hovers one.
 */
export function Capabilities({
  facts,
  from,
  compact = false,
}: {
  facts: Facts
  from: 'file' | 'publisher'
  compact?: boolean
}) {
  return (
    <span className={styles.tags} data-compact={compact}>
      {KINDS.map(({ key, name, icon: Icon }) => {
        const fact = facts[key]
        const said = saying(name, fact, from)
        return (
          <span
            key={key}
            className={styles.tag}
            data-cap={key}
            data-fact={fact}
            title={said}
            aria-label={said}
            role="img"
          >
            <Icon className={styles.icon} strokeWidth={1.8} aria-hidden />
            {!compact && (
              <span className={styles.name} aria-hidden>
                {name}
              </span>
            )}
            {fact === 'unknown' && (
              <span className={styles.unknown} aria-hidden>
                ?
              </span>
            )}
          </span>
        )
      })}
    </span>
  )
}

const KINDS: { key: keyof Facts; name: string; icon: LucideIcon }[] = [
  { key: 'vision', name: 'Vision', icon: Eye },
  { key: 'tools', name: 'Tools', icon: Wrench },
  { key: 'reasoning', name: 'Reasoning', icon: Brain },
  { key: 'audio', name: 'Audio', icon: AudioLines },
]

/** What one tag says to a person who hovers it, or to a screen reader. */
function saying(name: string, fact: Fact, from: 'file' | 'publisher'): string {
  switch (fact) {
    case 'yes':
      return from === 'file' ? `${name}: read from the file` : `${name}: stated by the publisher`
    case 'no':
      return `${name}: not in the file`
    case 'unknown':
      return from === 'file'
        ? `${name}: the file does not say`
        : `${name}: not stated by the publisher, so unknown`
  }
}
