import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { HealthCard } from './HealthCard';
import { FieldFocusProvider } from '../hooks/useFieldFocus';
import type { HealthItem } from '../api';
import type { ReactNode } from 'react';

const mockGoToSettings = vi.fn();
const mockOnDismiss = vi.fn();

function wrap(ui: ReactNode) {
  return render(<FieldFocusProvider>{ui}</FieldFocusProvider>);
}

function makeItem(over: Partial<HealthItem> = {}): HealthItem {
  return {
    id: 'api_url_missing',
    severity: 'warn',
    params: { id: 'api_url_missing' },
    action: { kind: 'go_to_settings', field: 'api_url' },
    dismissible: true,
    fingerprint: 'fp-1',
    ...over,
  };
}

describe('HealthCard', () => {
  beforeEach(() => {
    mockGoToSettings.mockReset();
    mockOnDismiss.mockReset();
  });

  it('renders nothing when items is empty', () => {
    const { container } = wrap(
      <HealthCard items={[]} onGoToSettings={mockGoToSettings} onDismiss={mockOnDismiss} />
    );
    expect(container.firstChild).toBeNull();
  });

  it('renders one row per item', () => {
    wrap(
      <HealthCard
        items={[
          makeItem({ id: 'api_url_missing' }),
          makeItem({
            id: 'gamelog_missing',
            params: { id: 'gamelog_missing' },
            fingerprint: 'fp-2',
          }),
        ]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    expect(screen.getAllByRole('listitem')).toHaveLength(2);
  });

  it('shows Dismiss only when dismissible', () => {
    wrap(
      <HealthCard
        items={[
          makeItem({
            id: 'auth_lost',
            severity: 'error',
            dismissible: false,
            params: { id: 'auth_lost' },
            fingerprint: 'fp-a',
          }),
          makeItem({
            id: 'update_available',
            severity: 'info',
            dismissible: true,
            params: { id: 'update_available', version: '0.4.1' },
            fingerprint: 'fp-b',
          }),
        ]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    expect(screen.getAllByRole('button', { name: /dismiss/i })).toHaveLength(1);
  });

  it('clicking a go_to_settings CTA calls onGoToSettings with the field', async () => {
    const user = userEvent.setup();
    wrap(
      <HealthCard items={[makeItem()]} onGoToSettings={mockGoToSettings} onDismiss={mockOnDismiss} />
    );
    await user.click(screen.getByRole('button', { name: /set up/i }));
    expect(mockGoToSettings).toHaveBeenCalledWith('api_url');
  });

  it('clicking Dismiss calls onDismiss with the item id', async () => {
    const user = userEvent.setup();
    wrap(
      <HealthCard
        items={[makeItem({ id: 'cookie_missing', params: { id: 'cookie_missing' }, fingerprint: 'fp-c' })]}
        onGoToSettings={mockGoToSettings}
        onDismiss={mockOnDismiss}
      />
    );
    await user.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(mockOnDismiss).toHaveBeenCalledWith('cookie_missing');
  });
});
