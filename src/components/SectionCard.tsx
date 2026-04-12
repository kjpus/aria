import type { PropsWithChildren, ReactNode } from 'react';

type SectionCardProps = PropsWithChildren<{
  eyebrow?: string;
  title?: string;
  actions?: ReactNode;
  hideHeader?: boolean;
}>;

export function SectionCard({
  eyebrow,
  title,
  actions,
  hideHeader = false,
  children,
}: SectionCardProps) {
  return (
    <section className="section-card">
      {!hideHeader ? (
        <header className="section-card__header">
          <div>
            {eyebrow ? <p className="section-card__eyebrow">{eyebrow}</p> : null}
            {title ? <h2>{title}</h2> : null}
          </div>
          {actions ? <div className="section-card__actions">{actions}</div> : null}
        </header>
      ) : null}
      <div className="section-card__body">{children}</div>
    </section>
  );
}
