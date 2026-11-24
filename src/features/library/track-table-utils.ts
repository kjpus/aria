export function columnWidthsEqual(
  left: Record<string, number>,
  right: Record<string, number>,
): boolean {
  const leftKeys = Object.keys(left).sort();
  const rightKeys = Object.keys(right).sort();

  if (leftKeys.length !== rightKeys.length) {
    return false;
  }

  for (let index = 0; index < leftKeys.length; index += 1) {
    const key = leftKeys[index];
    if (key !== rightKeys[index]) {
      return false;
    }
    if (left[key] !== right[key]) {
      return false;
    }
  }

  return true;
}

export function stringListsEqual(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }

  return true;
}

export function resolveContextMenuPosition(
  container: HTMLElement | null,
  clientX: number,
  clientY: number,
  menuWidth: number,
  menuHeight = 280,
) {
  if (!container) {
    return {
      x: Math.max(12, Math.min(clientX, window.innerWidth - menuWidth - 12)),
      y: Math.max(12, Math.min(clientY, window.innerHeight - menuHeight - 12)),
    };
  }

  const bounds = container.getBoundingClientRect();
  const padding = 12;
  const localX = clientX - bounds.left + container.scrollLeft;
  const localY = clientY - bounds.top + container.scrollTop;
  const minLeft = container.scrollLeft + padding;
  const maxLeft = Math.max(
    minLeft,
    container.scrollLeft + container.clientWidth - menuWidth - padding,
  );
  const minTop = container.scrollTop + padding;
  const maxTop = Math.max(
    minTop,
    container.scrollTop + container.clientHeight - menuHeight - padding,
  );

  return {
    x: Math.max(minLeft, Math.min(localX, maxLeft)),
    y: Math.max(minTop, Math.min(localY, maxTop)),
  };
}

export function getColumnWidthBounds(key: string): { min: number; max: number } {
  const min = ['track_number', 'disk_number', 'year', 'format', 'duration'].includes(key)
    ? 76
    : key === 'title'
      ? 160
      : 120;

  const max =
    key === 'title'
      ? 1500
      : key === 'album'
        ? 1300
        : key === 'file_name' || key === 'path'
          ? 1500
          : key === 'composer'
            ? 760
            : ['conductor', 'ensemble', 'soloist'].includes(key)
              ? 860
              : ['track_number', 'disk_number', 'year', 'format', 'duration'].includes(key)
                ? 240
                : 720;

  return { min, max };
}

export function clampColumnWidth(key: string, width: number): number {
  const { min, max } = getColumnWidthBounds(key);
  return Math.max(min, Math.min(max, Math.round(width)));
}

export function resizeAdjacentColumns(
  key: string,
  partnerKey: string,
  startWidth: number,
  startPartnerWidth: number,
  delta: number,
): Record<string, number> {
  const primaryBounds = getColumnWidthBounds(key);
  const partnerBounds = getColumnWidthBounds(partnerKey);
  const minimumDelta = Math.max(
    primaryBounds.min - startWidth,
    startPartnerWidth - partnerBounds.max,
  );
  const maximumDelta = Math.min(
    primaryBounds.max - startWidth,
    startPartnerWidth - partnerBounds.min,
  );
  const boundedDelta =
    maximumDelta < minimumDelta
      ? 0
      : Math.max(minimumDelta, Math.min(maximumDelta, delta));

  return {
    [key]: clampColumnWidth(key, startWidth + boundedDelta),
    [partnerKey]: clampColumnWidth(
      partnerKey,
      startPartnerWidth - boundedDelta,
    ),
  };
}

export function getDefaultColumnWidth(key: string): number {
  switch (key) {
    case 'track_number':
    case 'disk_number':
      return 96;
    case 'year':
    case 'format':
    case 'duration':
      return 110;
    case 'file_name':
      return 220;
    case 'path':
      return 360;
    case 'title':
      return 280;
    case 'album':
      return 260;
    case 'composer':
    case 'conductor':
    case 'ensemble':
    case 'soloist':
      return 220;
    default:
      return 180;
  }
}

export function buildColumnWeight(
  key: string,
  columnWidths: Record<string, number>,
  visibleColumns: string[],
): number {
  const total = visibleColumns.reduce(
    (sum, columnKey) =>
      sum + (columnWidths[columnKey] ?? getDefaultColumnWidth(columnKey)),
    0,
  );

  if (total <= 0) {
    return 100 / Math.max(visibleColumns.length, 1);
  }

  return ((columnWidths[key] ?? getDefaultColumnWidth(key)) / total) * 100;
}
