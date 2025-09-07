import { motion, AnimatePresence } from "framer-motion";
import { X, Sparkles, Loader2, OctagonX, PartyPopper } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useMemo } from "react";

export type ComicStage =
  | { stage: "queued" }
  | { stage: "parsing" }
  | { stage: "storyboarding" }
  | { stage: "prompting" }
  | { stage: "rendering"; completed: number; total: number }
  | { stage: "saving" }
  | { stage: "done" }
  | { stage: "failed"; error: string };

export type ComicJobStatus = {
  job_id: string;
  entry_id: string;
  style: string;
  stage: ComicStage;
  updated_at: string;
  result_image_path?: string | null;
  storyboard_text?: string | null;
};

type Props = {
  open: boolean;
  status: ComicJobStatus | null;
  onClose: () => void;
  onCancel: () => void;
};

export function ComicProgressModal({ open, status, onClose, onCancel }: Props) {
  const subtitle = useMemo(() => {
    if (!status) return "";
    const s = status.stage;
    if (s.stage === "rendering") return `Rendering ${s.completed}/${s.total} panels…`;
    if (s.stage === "failed") return `Bummer: ${s.error}`;
    if (s.stage === "done") return "All set! Your comic is ready.";
    const words: Record<string, string> = {
      queued: "Queued up…",
      parsing: "Parsing your vibes…",
      storyboarding: "Sketching the beats…",
      prompting: "Asking the muse (Ollama)…",
      saving: "Saving pixels…",
    } as const;
    // @ts-expect-error index
    return words[s.stage] || s.stage;
  }, [status]);

  const isActive = open && !!status;
  const isDone = status?.stage.stage === "done";
  const isFailed = status?.stage.stage === "failed";

  return (
    <AnimatePresence>
      {isActive ? (
        <motion.div
          className="fixed inset-0 z-[60] flex items-center justify-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <div className="absolute inset-0 bg-slate-900/50 backdrop-blur-sm" onClick={onClose} />

          <motion.div
            role="dialog"
            aria-modal="true"
            className={cn(
              "relative w-[680px] max-w-[92vw] rounded-2xl border border-slate-200 bg-white shadow-2xl overflow-hidden",
            )}
            initial={{ y: 24, scale: 0.98, opacity: 0 }}
            animate={{ y: 0, scale: 1, opacity: 1 }}
            exit={{ y: 16, scale: 0.98, opacity: 0 }}
            transition={{ type: "spring", stiffness: 300, damping: 24 }}
          >
            <div className="absolute right-3 top-3">
              <button
                className="inline-flex h-8 w-8 items-center justify-center rounded-full text-slate-500 hover:bg-slate-100"
                onClick={onClose}
                aria-label="Close"
              >
                <X className="h-4 w-4" />
              </button>
            </div>

            <div className="px-6 pb-4 pt-6">
              <div className="flex items-center gap-2">
                <div className="relative">
                  <div className="absolute -inset-1 rounded-xl bg-gradient-to-r from-purple-500/30 to-pink-500/30 blur" />
                  <div className="relative flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-r from-purple-500 to-pink-500 text-white">
                    {isDone ? <PartyPopper className="h-5 w-5" /> : isFailed ? <OctagonX className="h-5 w-5" /> : <Sparkles className="h-5 w-5" />}
                  </div>
                </div>
                <div className="">
                  <div className="text-lg font-semibold text-slate-900">Cartoonify in progress</div>
                  <div className="text-sm text-slate-500">{subtitle}</div>
                </div>
              </div>
            </div>

            <div className="px-6 pb-6">
              {/* Progress bar */}
              <div className="h-2 w-full overflow-hidden rounded-full bg-slate-100">
                <div
                  className={cn(
                    "h-full bg-gradient-to-r from-purple-500 to-pink-500 transition-all",
                    isFailed && "from-rose-500 to-rose-600",
                    isDone && "from-emerald-500 to-emerald-500"
                  )}
                  style={{ width: (() => {
                    const s = status?.stage;
                    if (!s) return "8%";
                    if (s.stage === "queued") return "8%";
                    if (s.stage === "parsing") return "16%";
                    if (s.stage === "storyboarding") return "28%";
                    if (s.stage === "prompting") return "42%";
                    if (s.stage === "rendering") return `${60 + Math.round((s.completed / s.total) * 30)}%`;
                    if (s.stage === "saving") return "94%";
                    if (s.stage === "done") return "100%";
                    if (s.stage === "failed") return "100%";
                    return "8%";
                  })() }}
                />
              </div>

              {/* Live storyboard */}
              <div className="mt-5 rounded-xl border border-slate-200 bg-slate-50/50 p-4">
                <div className="mb-1 flex items-center justify-between">
                  <div className="text-sm font-medium text-slate-700">Storyboard (live)</div>
                  <div className="text-xs tabular-nums text-slate-400">{status?.updated_at}</div>
                </div>
                <div className="max-h-44 overflow-auto rounded-md bg-white p-3 text-sm leading-relaxed text-slate-700">
                  {status?.storyboard_text ? (
                    <pre className="whitespace-pre-wrap font-mono text-[12px] text-slate-700">{status.storyboard_text}</pre>
                  ) : (
                    <div className="flex items-center gap-2 text-slate-400">
                      <Loader2 className="h-4 w-4 animate-spin" />
                      <span>Waiting for Ollama…</span>
                    </div>
                  )}
                </div>
              </div>

              {/* Result image or animated placeholder while rendering */}
              <div className="mt-5">
                <div className="text-sm font-medium text-slate-700">Preview</div>
                <div className="mt-2 overflow-hidden rounded-xl border border-slate-200 bg-white min-h-[180px] flex items-center justify-center">
                  {status?.result_image_path ? (
                    <img
                      src={convertFileSrc(status.result_image_path)}
                      className="block max-h-[280px] w-full object-contain"
                      alt="Generated comic preview"
                    />
                  ) : (
                    <div className="flex items-center gap-3 text-slate-500">
                      <div className="h-6 w-6 rounded-full border-2 border-slate-200 border-t-transparent animate-spin" />
                      <span className="text-sm">Brewing pixels…</span>
                    </div>
                  )}
                </div>
              </div>

              {/* Controls */}
              <div className="mt-6 flex items-center justify-end gap-2">
                {!isDone && !isFailed ? (
                  <Button onClick={onCancel} variant="secondary" size="sm">
                    Cancel
                  </Button>
                ) : null}
                <Button onClick={onClose} variant={isDone ? "primary" : "outline"} size="sm">
                  {isDone ? "Close" : "Hide"}
                </Button>
              </div>
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}


