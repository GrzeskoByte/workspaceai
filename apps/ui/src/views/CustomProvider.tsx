// Add or edit a custom model provider. The key goes to the keychain; opencode.json only gets the env name (R-20, R-34).
import { useState } from 'react';
import { useStore, setState, refreshSnap, toast, wsId } from '../store.ts';
import { patch, post } from '../api.ts';
import { CUSTOM_PRESETS } from '../../../../packages/shared/src/defaults.ts';
import type { Provider } from '../../../../packages/shared/src/types.ts';

export function CustomProvider() {
  const open = useStore((s) => s.customProvider);
  if (!open) return null;
  return <ProviderForm edit={open === true ? null : open} />;
}

function ProviderForm({ edit }: { edit: Provider | null }) {
  const [form, setForm] = useState(edit
    ? { id: edit.id, name: edit.name, url: edit.baseURL || '', env: edit.keyEnv || '', model: edit.models.map((m) => m.id).join(', '), key: '', pkg: edit.npm || '' }
    : { id: '', name: '', url: '', env: '', model: '', key: '', pkg: '@ai-sdk/openai-compatible' });
  const [test, setTest] = useState('');
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const applyPreset = (id: string) => {
    const p = CUSTOM_PRESETS.find((x) => x.id === id);
    if (p) setForm({ id: p.id, name: p.name, url: p.url, env: p.env, model: p.model, key: '', pkg: p.pkg });
  };
  const close = () => setState({ customProvider: false });
  const models = form.model.split(',').map((m) => m.trim()).filter(Boolean);
  const save = async () => {
    try {
      if (edit) await patch(`/api/ws/${wsId()}/providers/${edit.id}`, { name: form.name, url: form.url, models, key: form.key });
      else await post(`/api/ws/${wsId()}/providers`, form);
      await refreshSnap();
      close();
      toast(edit ? 'Provider updated.' : 'Provider added. The key is in the keychain, not in opencode.json.');
    } catch (e) { toast((e as Error).message, 'error'); }
  };
  const check = async () => {
    type Check = { ok: boolean; error?: string; models?: string[]; ms?: number };
    setTest('Testing…');
    const r: Check = await post<Check>(`/api/ws/${wsId()}/providers/test`, { id: edit?.id, url: form.url, key: form.key, model: models[0], pkg: form.pkg }).catch((e) => ({ ok: false, error: (e as Error).message }));
    setTest(r.ok ? (models[0] ? `Connected · ${models[0]} replied` : 'Connected · add a model id to test a real reply') + (r.ms ? ` · ${r.ms} ms` : '') : (r.error || 'No answer'));
  };
  return (
    <div className="scrim" onMouseDown={close}>
      <form className="card rise" role="dialog" aria-modal="true" aria-label="Custom provider" style={{ marginTop: 64, width: 480, maxWidth: 'calc(100vw - 24px)', padding: 22, display: 'flex', flexDirection: 'column', gap: 10 }} onMouseDown={(e) => e.stopPropagation()} onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <h2 className="disp" style={{ margin: 0, fontSize: 26 }}>{edit ? 'Edit ' + edit.name : 'Custom provider'}</h2>
        {!edit && <div className="row" style={{ flexWrap: 'wrap' }}>{CUSTOM_PRESETS.map((p) => <button key={p.id} type="button" className="chip" onClick={() => applyPreset(p.id)}>{p.label}</button>)}</div>}
        {!edit && <label className="field">ID<input className="inp" value={form.id} onChange={(e) => set('id', e.target.value)} required /></label>}
        <label className="field">Name<input className="inp" value={form.name} onChange={(e) => set('name', e.target.value)} required /></label>
        <label className="field">Base URL<input className="inp" value={form.url} onChange={(e) => set('url', e.target.value)} placeholder="https://…" required={!!edit} /></label>
        <label className="field">{edit ? 'Model ids, comma separated' : 'Model id'}<input className="inp" value={form.model} onChange={(e) => set('model', e.target.value)} required={!!edit} /></label>
        {!edit && <label className="field">Key env name<input className="inp" value={form.env} onChange={(e) => set('env', e.target.value)} /></label>}
        <label className="field">API key<span className="muted" style={{ fontWeight: 400 }}>{edit ? 'Leave empty to keep the stored key' : 'Stored in the OS keychain'}</span><input className="inp" type="password" value={form.key} onChange={(e) => set('key', e.target.value)} autoComplete="off" /></label>
        {test && <p className="muted" style={{ margin: 0 }}>{test}</p>}
        <div className="row"><button type="button" className="btn" onClick={() => void check()} disabled={test === 'Testing…'}>Test connection</button><span className="grow" /><button type="button" className="btn" onClick={close}>Cancel</button><button type="submit" className="btn btn-p">{edit ? 'Save' : 'Add'}</button></div>
      </form>
    </div>
  );
}
