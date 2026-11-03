// Memory for this workspace: approve, add, and forget facts, and visualize the Graphify graph (R-22, R-23).
import { MemoryPanel } from './MemoryPanel.tsx';

export function MemoryView() {
  return (
    <div className="page-pad" style={{ flex: '1 1 auto', overflowY: 'auto', padding: 28, display: 'flex', flexDirection: 'column', gap: 16 }}>
      <h1 className="disp" style={{ margin: 0, fontSize: 32 }}>Memory</h1>
      <MemoryPanel />
    </div>
  );
}
