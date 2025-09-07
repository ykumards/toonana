import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Settings = {
  gemini_api_key?: string | null;
  ollama_base_url?: string | null;
  default_ollama_model?: string | null;
  ollama_temperature?: number | null;
  ollama_top_p?: number | null;
  nano_banana_base_url?: string | null;
  nano_banana_api_key?: string | null;
};

type OllamaHealth = {
  ok: boolean;
  message?: string | null;
  models?: string[] | null;
};

type SettingsModalProps = {
  open: boolean;
  onClose: () => void;
  onSaved?: (settings: Settings) => void;
};

export function SettingsModal({ open, onClose, onSaved }: SettingsModalProps) {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [settings, setSettings] = useState<Settings>({});
  const [health, setHealth] = useState<OllamaHealth | null>(null);
  const [models, setModels] = useState<string[]>([]);

  const statusText = useMemo(() => {
    if (!health) return "";
    return health.ok ? "Ollama: running" : `Ollama: not running${health.message ? ` (${health.message})` : ""}`;
  }, [health]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        const s = await invoke<Settings>("get_settings");
        if (cancelled) return;
        setSettings(s || {});
        const h = await invoke<OllamaHealth>("ollama_health");
        if (cancelled) return;
        setHealth(h);
        const m = await invoke<string[]>("ollama_list_models");
        if (cancelled) return;
        setModels(m || []);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [open]);

  const handleRefreshModels = async () => {
    const h = await invoke<OllamaHealth>("ollama_health");
    setHealth(h);
    const m = await invoke<string[]>("ollama_list_models");
    setModels(m || []);
  };

  const handleSave = async () => {
    try {
      setSaving(true);
      const updated = await invoke<Settings>("update_settings", { settings });
      onSaved?.(updated);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/30" onClick={onClose} />
      <div className="relative w-full max-w-lg rounded-lg bg-white shadow-lg border border-journal-200">
        <div className="px-5 py-4 border-b border-journal-200 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Settings</h2>
          <button onClick={onClose} className="text-text-muted hover:text-text-primary">✕</button>
        </div>
        <div className="p-5 space-y-5">
          {loading ? (
            <div className="text-text-tertiary">Loading…</div>
          ) : (
            <>
              <div>
                <label className="block text-sm font-medium mb-1">Gemini API Key</label>
                <input
                  type="password"
                  value={settings.gemini_api_key || ""}
                  onChange={(e) => setSettings(s => ({ ...s, gemini_api_key: e.target.value }))}
                  placeholder="Paste your Gemini API key"
                  className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                />
                <div className="mt-1 text-xs text-text-tertiary">
                  Used for image generation by default. If Nano‑Banana is configured below, it will be used instead.
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium mb-1">Ollama Base URL</label>
                  <input
                    type="text"
                    value={settings.ollama_base_url || "http://127.0.0.1:11434"}
                    onChange={(e) => setSettings(s => ({ ...s, ollama_base_url: e.target.value }))}
                    placeholder="http://127.0.0.1:11434"
                    className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium mb-1">Default Ollama Model</label>
                  <div className="flex gap-2">
                    <select
                      value={settings.default_ollama_model || ""}
                      onChange={(e) => setSettings(s => ({ ...s, default_ollama_model: e.target.value }))}
                      className="flex-1 px-3 py-2 border border-journal-300 rounded-md bg-white"
                    >
                      <option value="">Select a model…</option>
                      {models.map(m => (
                        <option key={m} value={m}>{m}</option>
                      ))}
                    </select>
                    <button onClick={handleRefreshModels} className="px-3 py-2 border border-journal-300 rounded-md bg-journal-50">Refresh</button>
                  </div>
                  <div className="mt-1 text-xs text-text-tertiary">{statusText}</div>
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium mb-1">Nano-Banana Base URL</label>
                  <input
                    type="text"
                    value={settings.nano_banana_base_url || ""}
                    onChange={(e) => setSettings(s => ({ ...s, nano_banana_base_url: e.target.value }))}
                    placeholder="https://api.nano-banana.local"
                    className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                  />
                  <div className="mt-1 text-xs text-text-tertiary">Optional. Leave blank to use Gemini for images.</div>
                </div>
                <div>
                  <label className="block text-sm font-medium mb-1">Nano-Banana API Key</label>
                  <input
                    type="password"
                    value={settings.nano_banana_api_key || ""}
                    onChange={(e) => setSettings(s => ({ ...s, nano_banana_api_key: e.target.value }))}
                    placeholder="Optional"
                    className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                  />
                  <div className="mt-1 text-xs text-text-tertiary">Only required if your Nano‑Banana server enforces it.</div>
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium mb-1">Temperature</label>
                  <input
                    type="number"
                    step="0.1"
                    value={settings.ollama_temperature ?? 0}
                    onChange={(e) => setSettings(s => ({ ...s, ollama_temperature: Number(e.target.value) }))}
                    className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium mb-1">Top P</label>
                  <input
                    type="number"
                    step="0.05"
                    min={0}
                    max={1}
                    value={settings.ollama_top_p ?? 1}
                    onChange={(e) => setSettings(s => ({ ...s, ollama_top_p: Number(e.target.value) }))}
                    className="w-full px-3 py-2 border border-journal-300 rounded-md bg-white"
                  />
                </div>
              </div>
            </>
          )}
        </div>
        <div className="px-5 py-4 border-t border-journal-200 flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 rounded-md border border-journal-300 bg-white text-text-primary">Cancel</button>
          <button onClick={handleSave} disabled={saving} className="px-4 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600">{saving ? "Saving…" : "Save"}</button>
        </div>
      </div>
    </div>
  );
}


