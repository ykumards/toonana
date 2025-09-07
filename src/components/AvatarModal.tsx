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
  const [generating, setGenerating] = useState(false);
  const [genStep, setGenStep] = useState<number>(0);
  const [previews, setPreviews] = useState<string[]>([]);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [existingPath, setExistingPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const existingSrc = existingPath ? convertFileSrc(existingPath) : null;
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  const [selectedFilePreview, setSelectedFilePreview] = useState<string | null>(null);

  const toDataUrl = (b64: string) => (b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`);
  const preloadImage = (src: string) => new Promise<void>((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => reject(new Error("image load failed"));
    img.src = src;
  });
  // const sleep = (ms: number) => new Promise(res => setTimeout(res, ms));

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      try {
        setLoading(true);
        const s = await invoke<Settings>("get_settings");
        if (cancelled) return;
        setExistingPath(s?.avatar_image_path || null);
        setPreviews([]);
        setSelectedIdx(null);
        setError(null);
        setSelectedFilePreview(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [open]);

  const handleSave = async () => {
    try {
      setSaving(true);
      setError(null);
      const s = await invoke<Settings>("get_settings");
      const updated = { ...(s || {}), avatar_description: null } as Settings;
      await invoke<Settings>("update_settings", { settings: updated });
      if (selectedIdx != null) {
        const b64 = previews[selectedIdx];
        const raw = b64.startsWith("data:") ? b64.slice(b64.indexOf(",") + 1) : b64;
        const savedPath = await invoke<string>("save_avatar_image", { base64Png: raw });
        setExistingPath(savedPath);
      }
      onSaved?.(null);
      onClose();
    } catch (e) {
      console.error("Save avatar failed", e);
      const msg = String(e ?? "Unknown error");
      setError(`Save failed: ${msg}`);
    } finally {
      setSaving(false);
    }
  };

  const handleGenerate = async () => {
    try {
      setGenerating(true);
      setGenStep(0);
      setError(null);
      setPreviews([]);
      setSelectedIdx(null);
      if (!selectedFilePreview) {
        throw new Error("Please select a photo first");
      }
      setGenStep(1);
      // Start cartoonify job and poll
      const id = await invoke<string>("create_cartoonify_job", { dataUri: selectedFilePreview });
      // Poll loop
      const poll = async () => {
        try {
          const status = await invoke<any>("get_avatar_job_status", { jobId: id });
          const st = status?.stage?.stage as string | undefined;
          if (st === "rendering") {
            const completed = status.stage.completed as number | undefined;
            const total = status.stage.total as number | undefined;
            if (typeof completed === "number" && typeof total === "number" && total > 0) {
              const pct = Math.min(100, Math.max(0, Math.round((completed / total) * 100)));
              setGenStep(pct);
            }
            setTimeout(poll, 450);
            return;
          }
          if (st === "done") {
            const img = status?.image_base64 as string | undefined;
            if (!img) {
              setError("Generation failed: no image returned");
              setGenerating(false);
              setGenStep(0);
              return;
            }
            const dataUrl = toDataUrl(img);
            await preloadImage(dataUrl);
            setPreviews([dataUrl]);
            setSelectedIdx(0);
            setGenerating(false);
            setGenStep(0);
            return;
          }
          if (st === "failed") {
            const msg = status?.stage?.error as string | undefined;
            setError(`Generation failed: ${msg || "Unknown error"}`);
            setGenerating(false);
            setGenStep(0);
            return;
          }
          // queued or unknown -> keep polling
          setTimeout(poll, 500);
        } catch (e) {
          console.error("Avatar polling error", e);
          setError(`Generation failed: ${String(e)}`);
          setGenerating(false);
          setGenStep(0);
        }
      };
      poll();
    } catch (e) {
      console.error("Avatar generation failed", e);
      const msg = String(e ?? "Unknown error");
      const hint = msg.toLowerCase().includes("gemini api key")
        ? " – Set your Gemini API key in Settings."
        : "";
      setError(`Generation failed: ${msg}${hint}`);
    } finally {
      // keep generating true until polling resolves
    }
  };

  const handleFileChange = async (file: File | null) => {
    try {
      setError(null);
      setPreviews([]);
      setSelectedIdx(null);
      if (!file) { setSelectedFilePreview(null); return; }
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result || ""));
        reader.onerror = () => reject(new Error("Failed to read file"));
        reader.readAsDataURL(file);
      });
      setSelectedFilePreview(dataUrl);
    } catch (e) {
      console.error(e);
      setError("Could not read the selected file");
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/30" onClick={!generating ? onClose : undefined} />
      <div className="relative w-full max-w-2xl rounded-lg bg-slate-900 shadow-lg border border-slate-700">
        <div className="px-5 py-4 border-b border-slate-700 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-white">Your Character</h2>
          <button onClick={!generating ? onClose : undefined} disabled={generating} className="text-slate-400 hover:text-white disabled:opacity-50">✕</button>
        </div>
        <div className="p-5 space-y-4">
          {loading ? (
            <div className="text-slate-400">Loading…</div>
          ) : (
            <>
              <p className="text-sm text-slate-400">Upload a photo, then we'll cartoonify it into your avatar.</p>
              <div className="flex items-center gap-4">
                <label className="cursor-pointer inline-flex items-center gap-2 px-3 py-2 rounded-md border border-slate-700 bg-slate-800 text-white hover:bg-slate-700">
                  <input
                    type="file"
                    accept="image/*"
                    onChange={(e) => handleFileChange(e.target.files?.[0] || null)}
                    className="hidden"
                  />
                  <span>Choose photo…</span>
                </label>
                {selectedFilePreview && (
                  <button
                    onClick={() => setLightboxSrc(selectedFilePreview)}
                    className="relative border rounded-md overflow-hidden border-slate-700"
                    title="Click to expand selected photo"
                  >
                    <img src={selectedFilePreview} alt="Selected" className="w-16 h-16 object-cover" />
                  </button>
                )}
              </div>
              <div className="flex items-center justify-between gap-4">
                <div className="flex items-center gap-3">
                  {existingSrc ? (
                    <button
                      onClick={() => setLightboxSrc(existingSrc)}
                      className="relative border rounded-md overflow-hidden border-slate-700"
                      title="Click to expand current avatar"
                    >
                      <img src={existingSrc} alt="Current avatar" className="w-16 h-16 object-cover" />
                    </button>
                  ) : (
                    <div className="w-16 h-16 rounded-md border border-dashed border-slate-700 grid place-items-center text-xs text-slate-400">None</div>
                  )}
                  <div>
                    <div className="text-sm font-medium text-white">Current avatar</div>
                    <div className="text-xs text-slate-400">
                      {existingPath ? (
                        <>
                          Set · <button
                            onClick={async () => {
                              try {
                                setError(null);
                                await invoke("delete_avatar_image");
                                setExistingPath(null);
                              } catch (e) {
                                console.error("Delete avatar failed", e);
                                setError(`Delete failed: ${String(e ?? "Unknown error")}`);
                              }
                            }}
                            className="text-rose-300 hover:text-rose-400"
                            title="Remove avatar"
                          >
                            Remove
                          </button>
                        </>
                      ) : (
                        "Not set"
                      )}
                    </div>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <button onClick={handleGenerate} disabled={generating || !selectedFilePreview} className="px-3 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600 disabled:opacity-50">
                    {generating ? "Generating…" : "Cartoonify photo"}
                  </button>
                </div>
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
                      onClick={() => { setSelectedIdx(idx); setLightboxSrc(b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`); }}
                      className={`relative border rounded-md overflow-hidden ${selectedIdx === idx ? "ring-2 ring-blue-500" : "border-slate-700"}`}
                      title="Click to expand"
                    >
                      <img src={b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`} alt={`Preview ${idx+1}`} className="w-24 h-24 object-cover" />
                    </button>
                  ))}
                  <div className="text-xs text-slate-400">Preview not saved yet — click Save to keep it.</div>
                </div>
              )}
            </>
          )}
        </div>
        <div className="px-5 py-4 border-t border-slate-700 flex justify-end gap-2">
          <button onClick={!generating ? onClose : undefined} disabled={generating} className="px-4 py-2 rounded-md border border-slate-700 bg-slate-800 text-white hover:bg-slate-700 disabled:opacity-50">Cancel</button>
          <button onClick={handleSave} disabled={generating || saving || (previews.length > 0 && selectedIdx == null)} className="px-4 py-2 rounded-md bg-accent-500 text-white hover:bg-accent-600">{saving ? "Saving…" : "Save"}</button>
        </div>

        {generating && (
          <div className="absolute inset-0 bg-slate-900/80 backdrop-blur-sm flex items-center justify-center">
            <div className="flex flex-col items-center gap-2">
              <svg className="animate-spin h-6 w-6 text-accent-600" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
              </svg>
              <div className="text-sm text-slate-400">Generating avatar {genStep}%…</div>
              <div className="text-xs text-slate-400">This can take up to ~20 seconds</div>
            </div>
          </div>
        )}

        {lightboxSrc && (
          <div className="absolute inset-0 bg-black/70 flex items-center justify-center z-10" onClick={() => setLightboxSrc(null)}>
            <img
              src={lightboxSrc}
              alt="Preview"
              className="max-w-[90vw] max-h-[80vh] rounded-md border border-white/20 shadow-2xl"
            />
          </div>
        )}
      </div>
    </div>
  );
}
