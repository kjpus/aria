import { convertFileSrc } from '@tauri-apps/api/core';

export const isTauriRuntime =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function toLocalImageSrc(path: string | null): string | null {
  if (!path) {
    return null;
  }

  if (isTauriRuntime) {
    return convertFileSrc(path);
  }

  return `file:///${path.replace(/\\/g, '/')}`;
}
