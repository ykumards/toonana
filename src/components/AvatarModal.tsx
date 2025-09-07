import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
  const [previews, setPreviews] = useState<string[]>([]);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [existingPath, setExistingPath] = useState<string | null>(null);

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
        const savedPath = await invoke<string>("save_avatar_image", { base64Png: b64 });
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
      // Generate 3 variations sequentially for simplicity
      const items: string[] = [];
      for (let i = 0; i < 3; i++) {
        const prompt = avatar + (i === 0 ? "" : `, variation ${i+1}`);
        const img = await invoke<string>("generate_avatar_image", { prompt });
        items.push(img);
      }
      setPreviews(items);
      setSelectedIdx(0);
    } finally {
      setGenerating(false);
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/30" onClick={onClose} />
      <div className="relative w-full max-w-2xl rounded-lg bg-white shadow-lg border border-journal-200">
        <div className="px-5 py-4 border-b border-journal-200 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Your Character</h2>
          <button onClick={onClose} className="text-text-muted hover:text-text-primary">✕</button>
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
              <div className="flex items-center gap-2">
                <button onClick={handleGenerate} disabled={generating || !avatar.trim()} className="px-3 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600 disabled:opacity-50">
                  {generating ? "Generating…" : "Generate previews"}
                </button>
                {existingPath ? (
                  <span className="text-xs text-text-tertiary">Current avatar set</span>
                ) : null}
              </div>
              {previews.length > 0 && (
                <div className="grid grid-cols-3 gap-3">
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
                </div>
              )}
            </>
          )}
        </div>
        <div className="px-5 py-4 border-t border-journal-200 flex justify-end gap-2">
          <button onClick={onClose} className="px-4 py-2 rounded-md border border-journal-300 bg-white text-text-primary">Cancel</button>
          <button onClick={handleSave} disabled={saving || (previews.length > 0 && selectedIdx == null)} className="px-4 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600">{saving ? "Saving…" : "Save"}</button>
        </div>
      </div>
    </div>
  );
}


