import React from 'react';
import { describe, expect, it, vi, beforeAll } from 'vitest';
import { render, screen } from '@testing-library/react';
import { FlamegraphPane } from '../FlamegraphPane';

// Stub Tauri IPC — not available in jsdom
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(new ArrayBuffer(0)),
}));

// Stub apache-arrow table parse (returns empty table)
vi.mock('apache-arrow', () => ({
  tableFromIPC: vi.fn().mockReturnValue({ numRows: 0, getChildAt: () => null }),
}));

// PixiJS requires a real WebGL context; fall through to Canvas2dFallback stub
vi.mock('../../../render/PixiRenderer', () => ({
  PixiRenderer: vi.fn().mockImplementation(() => {
    throw new Error('WebGL not available in test');
  }),
}));

vi.mock('../../../render/Canvas2dFallback', () => ({
  Canvas2dFallback: vi.fn().mockImplementation(() => ({
    render: vi.fn(),
    hitTest: vi.fn(() => null),
    resize: vi.fn(),
    destroy: vi.fn(),
    rendererType: 'canvas2d',
  })),
}));

beforeAll(() => {
  // ResizeObserver polyfill for jsdom
  global.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };

  // requestAnimationFrame polyfill
  global.requestAnimationFrame = (cb: FrameRequestCallback) => {
    setTimeout(cb, 0);
    return 0;
  };
  global.cancelAnimationFrame = () => {};
});

describe('FlamegraphPane', () => {
  it('renders without crash', () => {
    render(
      <FlamegraphPane
        pid={null}
        tid={null}
        timeRange={null}
        onFrameFocus={vi.fn()}
      />,
    );
  });

  it('shows loading state while fetching', async () => {
    const { getByText } = render(
      <FlamegraphPane
        pid={null}
        tid={null}
        timeRange={null}
        onFrameFocus={vi.fn()}
      />,
    );
    expect(getByText('Loading…')).toBeTruthy();
  });
});
