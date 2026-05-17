import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { InferredBadge } from './InferredBadge';

describe('InferredBadge', () => {
  it('renders the "Inferred" label', () => {
    render(<InferredBadge confidence={0.75} />);
    expect(screen.getByText(/inferred/i)).toBeInTheDocument();
  });

  it('renders the confidence as a rounded percent', () => {
    render(<InferredBadge confidence={0.752} />);
    expect(screen.getByText('75%')).toBeInTheDocument();
  });

  it('clamps confidence into the [0, 1] range for the percent label', () => {
    const { rerender } = render(<InferredBadge confidence={-0.1} />);
    expect(screen.getByText('0%')).toBeInTheDocument();
    rerender(<InferredBadge confidence={1.5} />);
    expect(screen.getByText('100%')).toBeInTheDocument();
  });
});
