import type { CSSProperties, ReactNode } from 'react';
import { useEffect, useRef, useState } from 'react';

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

  useEffect(() => {
    const measure = () => {
      if (!outerRef.current || !innerRef.current) {
        return;
      }

      const nextOverflow = Math.max(
        0,
        innerRef.current.scrollWidth - outerRef.current.clientWidth,
      );
      setOverflow(nextOverflow);
    };

    measure();

    const resizeObserver =
      typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(() => measure())
        : null;

    if (resizeObserver && outerRef.current && innerRef.current) {
      resizeObserver.observe(outerRef.current);
      resizeObserver.observe(innerRef.current);
    }

    window.addEventListener('resize', measure);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener('resize', measure);
    };
  }, [text]);

  const style = {
    '--scroll-distance': `${overflow}px`,
    '--scroll-duration': `${Math.max(2.25, overflow / speed)}s`,
  } as CSSProperties;

  return (
    <div
      className={`hover-scroll ${overflow > 0 ? 'hover-scroll--active' : ''} ${className ?? ''}`.trim()}
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
