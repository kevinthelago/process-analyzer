import React from 'react';
import { describe, expect, it, vi, beforeAll } from 'vitest';
import { render } from '@testing-library/react';
import { TimelinePane } from '../TimelinePane';

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

describe('TimelinePane', () => {
  it('renders without crash', () => {
    render(
      <TimelinePane
        pid={null}
        tid={null}
        onTimeRangeChange={vi.fn()}
        selectedTimeRange={null}
      />,
    );
  });

  it('shows loading indicator initially', () => {
    const { getByText } = render(
      <TimelinePane
        pid={null}
        tid={null}
        onTimeRangeChange={vi.fn()}
        selectedTimeRange={null}
      />,
    );
    expect(getByText('Loading…')).toBeTruthy();
  });
});
