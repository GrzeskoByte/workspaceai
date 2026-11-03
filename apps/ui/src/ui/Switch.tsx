export function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: () => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" className="sw" role="switch" aria-checked={on} aria-label={label} disabled={disabled} onClick={(e) => { e.preventDefault(); e.stopPropagation(); onChange(); }}>
      <span />
    </button>
  );
}
