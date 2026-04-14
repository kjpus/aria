import { forwardRef, useImperativeHandle, useRef } from 'react';
import type { ComponentPropsWithoutRef, MouseEvent } from 'react';

type ClearableInputProps = Omit<ComponentPropsWithoutRef<'input'>, 'type'> & {
  onClear?: () => void;
};

export const ClearableInput = forwardRef<HTMLInputElement, ClearableInputProps>(
  function ClearableInput({ className, onClear, value, ...props }, forwardedRef) {
    const inputRef = useRef<HTMLInputElement>(null);
    const hasValue = String(value ?? '').length > 0;

    useImperativeHandle(forwardedRef, () => inputRef.current as HTMLInputElement, []);

    function handleClear(event: MouseEvent<HTMLButtonElement>) {
      event.preventDefault();
      onClear?.();
      inputRef.current?.focus();
    }

    return (
      <div className={`clearable-input${className ? ` ${className}` : ''}`}>
        <input ref={inputRef} type="text" value={value} {...props} />
        {hasValue ? (
          <button
            aria-label="Clear filter"
            className="clearable-input__clear"
            onClick={handleClear}
            type="button"
          >
            ×
          </button>
        ) : null}
      </div>
    );
  },
);
