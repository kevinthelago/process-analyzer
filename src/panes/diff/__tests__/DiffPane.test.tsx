import React from 'react';
import { describe, expect, it, vi, beforeAll } from 'vitest';
import { render, screen } from '@testing-library/react';
import { DiffPane } from '../DiffPane';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(new ArrayBuffer(0)),
}));

vi.mock('apache-arrow', () => ({
  tableFromIPC: vi.fn().mockReturnValue({ numRows: 0, getChildAt: () => null }),
}));

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
  global.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  global.requestAnimationFrame = (cb: FrameRequestCallback) => {
    setTimeout(cb, 0);
    return 0;
  };
  global.cancelAnimationFrame = () => {};
});

describe('DiffPane', () => {
  it('shows empty state when no trace IDs provided', () => {
    const { getByText } = render(
      <DiffPane
        baselineTraceId={null}
        regressionTraceId={null}
        pid={null}
        tid={null}
        timeRange={null}
      />,
    );
    expect(getByText(/Select a baseline/)).toBeTruthy();
  });

  it('renders swap button', () => {
    const { getByText } = render(
      <DiffPane
        baselineTraceId={null}
        regressionTraceId={null}
        pid={null}
        tid={null}
        timeRange={null}
      />,
    );
    expect(getByText('Swap baseline')).toBeTruthy();
  });
});
