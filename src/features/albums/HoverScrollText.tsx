import type { CSSProperties, ReactNode } from 'react';
import { useCallback, useEffect, useRef, useState } from 'react';

type HoverScrollTextProps = {
  text: string;
  className?: string;
  speed?: number;
  children?: ReactNode;
};

export function HoverScrollText({
  text,
  className,
  speed = 28,
  children,
}: HoverScrollTextProps) {
  const outerRef = useRef<HTMLDivElement>(null);
  const innerRef = useRef<HTMLSpanElement>(null);
  const [overflow, setOverflow] = useState(0);

  const measure = useCallback(() => {
    if (!outerRef.current || !innerRef.current) {
      return;
    }

    const diff = innerRef.current.scrollWidth - outerRef.current.clientWidth;
    const nextOverflow = diff > 1 ? Math.ceil(diff) : 0;
    setOverflow((current) => (current === nextOverflow ? current : nextOverflow));
  }, []);

  useEffect(() => {
    setOverflow(0);
  }, [text]);

  const style = {
    '--scroll-distance': `${overflow}px`,
    '--scroll-duration': `${Math.max(2.25, overflow / speed)}s`,
  } as CSSProperties;

  return (
    <div
      className={`hover-scroll ${overflow > 0 ? 'hover-scroll--active' : ''} ${className ?? ''}`.trim()}
      onFocus={measure}
      onPointerEnter={measure}
      ref={outerRef}
      style={style}
      title={text}
    >
      <span className="hover-scroll__inner" ref={innerRef}>
        {children ?? text}
      </span>
    </div>
  );
}
