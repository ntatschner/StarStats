import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import { SubmissionsPane } from './SubmissionsPane';

const mockedInvoke = vi.mocked(invoke);

const baseRow = {
  id: 'row_1',
  raw_line: 'Jim opened the door',
  timestamp: '2026-05-17T12:00:00Z',
  shell_tag: 'Door',
  partial_structured: {},
  context_before: [],
  context_after: [],
  game_build: null,
  channel: 'channel_live' as const,
  interest_score: 70,
  shape_hash: 'sh_a',
  occurrence_count: 3,
  first_seen: '2026-05-17T12:00:00Z',
  last_seen: '2026-05-17T12:00:00Z',
  detected_pii: [],
  dismissed: false,
};

beforeEach(() => {
  mockedInvoke.mockReset();
});

describe('SubmissionsPane', () => {
  it('lists unknown lines from the Tauri bridge', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_unknown_lines') return [baseRow];
      return null;
    });
    render(<SubmissionsPane />);
    expect(await screen.findByText(/sh_a/)).toBeInTheDocument();
    expect(screen.getByText('Jim opened the door')).toBeInTheDocument();
  });

  it('invokes submit_unknown_lines on Submit click', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_unknown_lines') return [baseRow];
      if (cmd === 'submit_unknown_lines')
        return { accepted: 1, deduped: 0, ids: ['1'] };
      return null;
    });
    render(<SubmissionsPane />);
    await screen.findByText(/sh_a/);
    await userEvent.click(screen.getByRole('button', { name: /submit/i }));
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith(
        'submit_unknown_lines',
        expect.objectContaining({
          payloads: expect.arrayContaining([
            expect.objectContaining({
              shape_hash: 'sh_a',
              raw_examples: ['Jim opened the door'],
              channel: 'channel_live',
              occurrence_count: 3,
            }),
          ]),
        })
      );
    });
  });

  it('invokes dismiss_unknown_line on Dismiss click', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_unknown_lines') return [baseRow];
      return null;
    });
    render(<SubmissionsPane />);
    await screen.findByText(/sh_a/);
    await userEvent.click(screen.getByRole('button', { name: /dismiss/i }));
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith('dismiss_unknown_line', {
        shapeHash: 'sh_a',
      });
    });
  });

  it('reports count changes to the parent via onCountChange', async () => {
    mockedInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'list_unknown_lines') return [baseRow, { ...baseRow, shape_hash: 'sh_b' }];
      return null;
    });
    const onCountChange = vi.fn();
    render(<SubmissionsPane onCountChange={onCountChange} />);
    await waitFor(() => expect(onCountChange).toHaveBeenCalledWith(2));
  });
});
