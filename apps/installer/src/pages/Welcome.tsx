import { ArrowRight } from 'lucide-react';
import { Button, Logo, Notice } from '@demido/ui';

import type { Context } from '../types';
import styles from '../Setup.module.css';

export function WelcomePage({ ctx, onNext }: { ctx: Context; onNext: () => void }) {
  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <div className={styles.hero}>
          <Logo size={56} className={styles.heroLogo} />
          <h1 className={styles.title}>Welcome to Demido Studio</h1>
          <p className={styles.lead}>
            An AI workspace that runs on your own computer. Setup downloads everything it needs and picks what fits your
            hardware, so it works as soon as it opens.
          </p>
        </div>
        <ul className={styles.list}>
          <li>An AI runtime matched to your graphics card</li>
          <li>Python and Node.js, used by the assistant's tools (analysis, market data)</li>
          <li>A first model sized for your computer, from the Qwen or Gemma family</li>
        </ul>
        {ctx.existing && (
          <Notice tone="info" title={`Demido Studio ${ctx.existing.version} is already installed`}>
            Continuing updates it in {ctx.existing.dir}. Your chats, models and settings are kept.
          </Notice>
        )}
        {!ctx.hasPayload && (
          <Notice tone="warning" title="Development build" className={styles.spacerTop}>
            This setup has no app inside it, so it only installs the runtimes.
          </Notice>
        )}
      </div>
      <div className={styles.footer}>
        <span className={styles.footerInfo}>You need an internet connection during setup.</span>
        <Button variant="primary" size="lg" iconRight={ArrowRight} onClick={onNext}>
          Get started
        </Button>
      </div>
    </div>
  );
}
