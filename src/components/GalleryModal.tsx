import { AnimatePresence, motion } from "framer-motion";
import { X, CalendarDays } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "./ui/button";
import { cn } from "@/lib/utils";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

type ComicItem = {
  entry_id: string;
  image_path: string;
  created_at: string;
};

type ComicsByDay = {
  date: string; // YYYY-MM-DD
  comics: ComicItem[];
};

type Props = {
  open: boolean;
  onClose: () => void;
  onSelect?: (entryId: string) => void;
};

export function GalleryModal({ open, onClose, onSelect }: Props) {
  const [days, setDays] = useState<ComicsByDay[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lightbox, setLightbox] = useState<ComicItem | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<ComicsByDay[]>("list_comics_by_day", { limitDays: 120 })
      .then((res) => {
        if (!cancelled) setDays(res);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  return (
    <AnimatePresence>
      {open ? (
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
              "relative w-[1000px] max-w-[96vw] max-h-[90vh] overflow-y-auto rounded-2xl border border-slate-200 bg-white shadow-2xl",
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
                    <CalendarDays className="h-5 w-5" />
                  </div>
                </div>
                <div className="">
                  <div className="text-lg font-semibold text-slate-900">Gallery</div>
                  <div className="text-sm text-slate-500">Browse comics by day</div>
                </div>
              </div>
            </div>

            <div className="px-6 pb-6">
              {loading ? (
                <div className="flex h-32 items-center justify-center text-slate-500">Loading…</div>
              ) : error ? (
                <div className="rounded-md border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700">{error}</div>
              ) : days && days.length > 0 ? (
                <div className="space-y-8">
                  {days.map((d) => (
                    <div key={d.date}>
                      <div className="mb-2 text-sm font-semibold text-slate-700">{d.date}</div>
                      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-3">
                        {d.comics.map((c) => (
                          <button
                            key={c.image_path}
                            onClick={() => setLightbox(c)}
                            className="group relative overflow-hidden rounded-lg border border-slate-200 bg-white hover:shadow-md transition-shadow"
                            title={c.created_at}
                          >
                            <img
                              src={convertFileSrc(c.image_path)}
                              className="block aspect-[4/3] w-full object-cover"
                              alt="Comic"
                            />
                            <div className="absolute inset-x-0 bottom-0 h-8 bg-gradient-to-t from-black/50 to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
                          </button>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="flex h-32 items-center justify-center text-slate-500">No comics yet</div>
              )}
            </div>

            <div className="px-6 pb-6 flex items-center justify-end">
              <Button onClick={onClose} variant="outline" size="sm">Close</Button>
            </div>
          </motion.div>
        </motion.div>
      ) : null}

      {/* Lightbox */}
      {open && lightbox ? (
        <motion.div
          className="fixed inset-0 z-[70] flex items-center justify-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <div className="absolute inset-0 bg-black/80" onClick={() => setLightbox(null)} />
          <div className="relative max-w-[96vw] max-h-[92vh]">
            <img
              src={convertFileSrc(lightbox.image_path)}
              className="max-w-[96vw] max-h-[80vh] object-contain rounded-lg shadow-2xl"
              alt="Comic"
            />
            <div className="mt-3 flex items-center justify-end gap-2">
              {onSelect ? (
                <Button size="sm" variant="primary" onClick={() => { onSelect(lightbox.entry_id); setLightbox(null); }}>Open entry</Button>
              ) : null}
              <Button size="sm" variant="secondary" onClick={() => setLightbox(null)}>Close</Button>
            </div>
          </div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}


