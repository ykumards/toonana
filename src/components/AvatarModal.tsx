import { useEffect, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

type Settings = {
  avatar_description?: string | null;
  avatar_image_path?: string | null;
};

type AvatarModalProps = {
  open: boolean;
  onClose: () => void;
  onSaved?: (avatarDescription: string | null | undefined) => void;
};

export function AvatarModal({ open, onClose, onSaved }: AvatarModalProps) {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [avatar, setAvatar] = useState<string>("");
  const [generating, setGenerating] = useState(false);
  const [genStep, setGenStep] = useState<number>(0);
  const TOTAL_VARIATIONS = 1;
  const [previews, setPreviews] = useState<string[]>([]);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [existingPath, setExistingPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const existingSrc = existingPath ? convertFileSrc(existingPath) : null;

  const toDataUrl = (b64: string) => (b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`);
  const preloadImage = (src: string) => new Promise<void>((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("image load failed"));
    img.src = src;
  });

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        const s = await invoke<Settings>("get_settings");
        if (cancelled) return;
        setAvatar(s?.avatar_description || "");
        setExistingPath(s?.avatar_image_path || null);
        setPreviews([]);
        setSelectedIdx(null);
        setError(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [open]);

  const handleSave = async () => {
    try {
      setSaving(true);
      const s = await invoke<Settings>("get_settings");
      const updated = { ...(s || {}), avatar_description: avatar } as Settings;
      await invoke<Settings>("update_settings", { settings: updated });
      if (selectedIdx != null) {
        const b64 = previews[selectedIdx];
        const savedPath = await invoke<string>("save_avatar_image", { base64_png: b64 });
        setExistingPath(savedPath);
      }
      onSaved?.(avatar);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const handleGenerate = async () => {
    try {
      setGenerating(true);
      setGenStep(0);
      setError(null);
      // Generate a single preview per attempt
      const items: string[] = [];
      const prompt = avatar;
      setGenStep(1);
      try {
        const img = await invoke<string>("generate_avatar_image", { prompt });
        items.push(img);
      } catch (e) {
        console.error("Avatar generation failed", e);
      }
      if (items.length === 0) {
        setError("Generation failed: no previews produced. Check Settings for your Gemini API key or Nano‑Banana config.");
        return;
      }
      const dataUrls = items.map(toDataUrl);
      // Ensure previews are actually decoded before we hide the loader
      await Promise.all(dataUrls.map(preloadImage));
      setPreviews(dataUrls);
      setSelectedIdx(0);
    } catch (e) {
      console.error("Avatar generation failed", e);
      const msg = String(e ?? "Unknown error");
      const hint = msg.toLowerCase().includes("gemini api key")
        ? " – Set your Gemini API key in Settings."
        : "";
      setError(`Generation failed: ${msg}${hint}`);
    } finally {
      setGenerating(false);
      setGenStep(0);
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/30" onClick={!generating ? onClose : undefined} />
      <div className="relative w-full max-w-2xl rounded-lg bg-white shadow-lg border border-journal-200">
        <div className="px-5 py-4 border-b border-journal-200 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Your Character</h2>
          <button onClick={!generating ? onClose : undefined} disabled={generating} className="text-text-muted hover:text-text-primary disabled:opacity-50">✕</button>
        </div>
        <div className="p-5 space-y-4">
          {loading ? (
            <div className="text-text-tertiary">Loading…</div>
          ) : (
            <>
              <p className="text-sm text-text-tertiary">
                Describe yourself as a stylized character. We'll use this as your avatar in comics.
                Mention hair, clothing, vibe, key accessories, and any defining traits.
              </p>
              <textarea
                value={avatar}
                onChange={(e) => setAvatar(e.target.value)}
                placeholder="e.g., A cheerful 30‑something with short wavy hair, round glasses, cozy earth‑tone sweater, denim jacket with enamel pins, always carrying a sketchbook; warm, curious vibe."
                className="w-full min-h-40 px-3 py-2 border border-journal-300 rounded-md bg-white"
              />
              <div className="text-xs text-text-tertiary">
                Tip: Keep it concise but evocative (2‑4 sentences). We'll integrate this into prompts later.
              </div>
              {existingSrc && (
                <div className="flex items-center gap-3">
                  <div className="text-sm font-medium">Current avatar</div>
                  <img src={existingSrc} alt="Current avatar" className="w-16 h-16 rounded-md object-cover border border-journal-300" />
                </div>
              )}
              <div className="flex items-center gap-2">
                <button onClick={handleGenerate} disabled={generating || !avatar.trim()} className="px-3 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600 disabled:opacity-50">
                  {generating ? "Generating…" : "Generate preview"}
                </button>
                {existingPath ? (
                  <span className="text-xs text-text-tertiary">Current avatar set</span>
                ) : null}
              </div>
              {error && (
                <div className="text-sm text-rose-600 bg-rose-50 border border-rose-200 rounded-md px-3 py-2">
                  {error}
                </div>
              )}
              {previews.length > 0 && (
                <div className="grid grid-cols-1 gap-3">
                  {previews.map((b64, idx) => (
                    <button
                      key={idx}
                      onClick={() => setSelectedIdx(idx)}
                      className={`relative border rounded-md overflow-hidden ${selectedIdx === idx ? "ring-2 ring-accent-500" : "border-journal-300"}`}
                      title="Click to select"
                    >
                      <img src={b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`} alt={`Preview ${idx+1}`} className="w-full h-40 object-cover" />
                    </button>
                  ))}
                  <div className="text-xs text-text-tertiary">Preview not saved yet — click Save to keep it.</div>
                </div>
              )}
            </>
          )}
        </div>
        <div className="px-5 py-4 border-t border-journal-200 flex justify-end gap-2">
          <button onClick={!generating ? onClose : undefined} disabled={generating} className="px-4 py-2 rounded-md border border-journal-300 bg-white text-text-primary disabled:opacity-50">Cancel</button>
          <button onClick={handleSave} disabled={generating || saving || (previews.length > 0 && selectedIdx == null)} className="px-4 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600">{saving ? "Saving…" : "Save"}</button>
        </div>

        {generating && (
          <div className="absolute inset-0 bg-white/80 backdrop-blur-sm flex items-center justify-center">
            <div className="flex flex-col items-center gap-2">
              <svg className="animate-spin h-6 w-6 text-accent-600" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
              </svg>
              <div className="text-sm text-text-tertiary">Generating avatar {genStep}/{TOTAL_VARIATIONS}…</div>
              <div className="text-xs text-text-tertiary">This can take up to ~20 seconds</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}


