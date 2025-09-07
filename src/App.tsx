import { useEffect, useState, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Save, Sparkles, Settings, Check, Loader2, Menu, X, CalendarDays, User } from "lucide-react";
import { EntriesSidebar } from "./components/EntriesSidebar";
import { MarkdownEditor } from "./components/MarkdownEditor";
import { SettingsModal } from "./components/SettingsModal";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useAutosave } from "./hooks/useAutosave";
import { Button } from "./components/ui/button";
import { motion } from "framer-motion";
import "./App.css";
import { ComicProgressModal } from "./components/ComicProgressModal";
import { GalleryModal } from "./components/GalleryModal";
import { AvatarModal } from "./components/AvatarModal";

type OllamaHealth = {
  ok: boolean;
  message?: string | null;
  models?: string[] | null;
};

type Entry = {
  id: string;
  created_at: string;
  updated_at: string;
  body_cipher: number[];
  mood?: string | null;
  tags?: unknown | null;
};

type EntryUpsert = {
  id?: string;
  body_cipher: number[];
  mood?: string | null;
  tags?: unknown | null;
};

type EntryListItem = {
  id: string;
  created_at: string;
  updated_at: string;
  body_preview?: string | null;
  mood?: string | null;
  tags?: unknown | null;
};

type ComicStage =
  | { stage: "queued" }
  | { stage: "parsing" }
  | { stage: "storyboarding" }
  | { stage: "prompting" }
  | { stage: "rendering"; completed: number; total: number }
  | { stage: "saving" }
  | { stage: "done" }
  | { stage: "failed"; error: string };

type ComicJobStatus = {
  job_id: string;
  entry_id: string;
  style: string;
  stage: ComicStage;
  updated_at: string;
  result_image_path?: string | null;
  storyboard_text?: string | null;
};

function useInit() {
  const { data } = useQuery({
    queryKey: ["health"],
    queryFn: async () => invoke<{ ok: boolean; has_vault_key: boolean }>("health"),
  });
  useEffect(() => {
    if (!data?.has_vault_key) {
      invoke("init_vault");
    }
  }, [data]);
  return data;
}

function useEntries() {
  return useQuery({
    queryKey: ["entries"],
    queryFn: async () => invoke<EntryListItem[]>("db_list_entries", { p: { limit: 100, offset: 0 } }),
  });
}

function encrypt(plaintext: string) {
  return invoke<number[]>("encrypt", { plaintext });
}

function decrypt(cipher: number[]) {
  return invoke<string>("decrypt", { cipher });
}

export default function App() {
  useInit();
  const qc = useQueryClient();
  const { data: entries, isLoading } = useEntries();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [body, setBody] = useState("");
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const [editorMode, setEditorMode] = useState<"edit" | "preview">("edit");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [comicJobId, setComicJobId] = useState<string | null>(null);
  const [comicStatus, setComicStatus] = useState<ComicJobStatus | null>(null);
  const [isPolling, setIsPolling] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [progressOpen, setProgressOpen] = useState(false);
  const [ollamaHealth, setOllamaHealth] = useState<OllamaHealth | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [galleryOpen, setGalleryOpen] = useState(false);
  const [avatarOpen, setAvatarOpen] = useState(false);

  const upsert = useMutation({
    mutationFn: async () => {
      const body_cipher = await encrypt(body);
      const payload: EntryUpsert = { id: selectedId ?? undefined, body_cipher };
      return invoke<Entry>("db_upsert_entry", { entry: payload });
    },
    onSuccess: async (e) => {
      setSelectedId(e.id);
      setHasUnsavedChanges(false);
      await qc.invalidateQueries({ queryKey: ["entries"] });
    },
  });

  const removeEntry = useMutation({
    mutationFn: async (id: string) => {
      return invoke<void>("db_delete_entry", { id });
    },
    onMutate: async (id: string) => {
      await qc.cancelQueries({ queryKey: ["entries"] });
      const prev = qc.getQueryData<EntryListItem[] | undefined>(["entries"]);
      if (prev) {
        qc.setQueryData<EntryListItem[]>(["entries"], prev.filter(e => e.id !== id));
      }
      return { prev } as { prev?: EntryListItem[] };
    },
    onError: (err, _id, ctx) => {
      if (ctx?.prev) qc.setQueryData(["entries"], ctx.prev);
      window.alert(`Delete failed: ${String(err)}`);
    },
    onSuccess: async (_, id) => {
      if (selectedId === id) {
        setSelectedId(null);
        setBody("");
        setHasUnsavedChanges(false);
      }
      await qc.invalidateQueries({ queryKey: ["entries"] });
    }
  });

  const loadEntry = async (id: string) => {
    // Save current entry if there are unsaved changes
    if (hasUnsavedChanges) {
      await upsert.mutateAsync();
    }

    const e = await invoke<Entry>("db_get_entry", { id });
    setSelectedId(e.id);
    try {
      const text = await decrypt(e.body_cipher);
      setBody(text);
    } catch {
      setBody("");
    }
    setHasUnsavedChanges(false);
  };

  const startNew = async () => {
    // Save current entry if there are unsaved changes
    if (hasUnsavedChanges) {
      await upsert.mutateAsync();
    }

    setSelectedId(null);
    setBody("");
    setHasUnsavedChanges(false);
  };

  const makeComic = async () => {
    if (!selectedId) {
      console.warn("Make Comic pressed but no entry selected");
      return;
    }
    // Open modal immediately so users see progress even if backend call is slow
    setProgressOpen(true);
    console.log("Creating comic job for entry", selectedId);
    try {
      const job = await invoke<string>("create_comic_job", { entryId: selectedId, style: "nano-banana" });
      console.log("Comic job created", job);
      setComicJobId(job);
      setIsPolling(true);
    } catch (e) {
      console.error("Failed to create comic job", e);
      // Surface failure in the modal
      setComicStatus({
        job_id: "local",
        entry_id: selectedId,
        style: "nano-banana",
        stage: { stage: "failed", error: String(e) },
        updated_at: new Date().toISOString(),
        result_image_path: null,
        storyboard_text: null,
      });
      setIsPolling(false);
    }
  };

  // Light-weight health polling for Ollama
  useEffect(() => {
    let stopped = false;
    const check = async () => {
      try {
        const h = await invoke<OllamaHealth>("ollama_health");
        if (!stopped) setOllamaHealth(h);
      } catch {
        if (!stopped) setOllamaHealth({ ok: false, message: "unreachable", models: null });
      }
    };
    check();
    const id = setInterval(check, 5000);
    return () => {
      stopped = true;
      clearInterval(id);
    };
  }, []);

  useEffect(() => {
    if (!comicJobId || !isPolling) return;
    let stopped = false;
    const poll = async () => {
      try {
        const status = await invoke<ComicJobStatus>("get_comic_job_status", { jobId: comicJobId });
        if (stopped) return;
        setComicStatus(status);
        try {
          const s = status.stage as ComicStage;
          console.log("Comic status", s);
        } catch {}
        const stage = status.stage as ComicStage;
        if (stage.stage === "done" || stage.stage === "failed") {
          setIsPolling(false);
          return;
        }
      } catch (e) {
        // stop polling on error
        console.error("Polling error", e);
        setIsPolling(false);
        return;
      }
      setTimeout(poll, 400);
    };
    poll();
    return () => { stopped = true; };
  }, [comicJobId, isPolling]);

  

  // Track changes
  const handleBodyChange = (newBody: string) => {
    setBody(newBody);
    setHasUnsavedChanges(true);
  };

  // Auto-save on blur or after delay
  const handleSave = () => {
    if (hasUnsavedChanges) {
      upsert.mutate();
    }
  };

  // Keyboard shortcuts
  useKeyboardShortcuts({
    onSave: handleSave,
    onNewEntry: startNew,
    onTogglePreview: () => setEditorMode(prev => prev === "edit" ? "preview" : "edit"),
    onSearch: () => searchInputRef.current?.focus()
  });

  // Auto-save after 3 seconds of inactivity
  useAutosave({
    onSave: handleSave,
    hasChanges: hasUnsavedChanges,
    delay: 3000,
    enabled: true
  });

  return (
    <div className="h-screen w-full overflow-hidden dark bg-background">
      <div className="flex h-full relative">
        {/* Mobile overlay */}
        {sidebarOpen && (
          <div
            className="fixed inset-0 bg-black/50 z-10 sm:hidden"
            onClick={() => setSidebarOpen(false)}
          />
        )}
        
        {/* Sidebar */}
        <div className={`${sidebarOpen ? 'translate-x-0' : '-translate-x-full'} sm:translate-x-0 transition-transform duration-200 w-64 md:w-72 lg:w-80 flex-shrink-0 z-20 fixed sm:relative h-full`}>
          <EntriesSidebar
            entries={entries ?? []}
            selectedId={selectedId}
            isLoading={isLoading}
            onEntrySelect={(id) => {
              loadEntry(id);
              setSidebarOpen(false);
            }}
            onNewEntry={() => {
              startNew();
              setSidebarOpen(false);
            }}
            searchInputRef={searchInputRef}
            onDeleteEntry={async (id) => {
              console.log("Deleting entry", id);
              try {
                await removeEntry.mutateAsync(id);
                console.log("Delete request sent", id);
              } catch (e) {
                console.error("Delete failed", e);
              }
            }}
          />
        </div>

        {/* Main Editor */}
        <motion.div 
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.4, delay: 0.1, ease: [0.4, 0, 0.2, 1] }}
          className="flex-1 flex flex-col min-w-0 sm:ml-0 bg-background"
        >
          {/* Modern Editor Header */}
          <div className="border-b border-border bg-card/95 backdrop-blur supports-[backdrop-filter]:bg-card/60">
            <div className="px-4 sm:px-6 lg:px-8 py-3 sm:py-4">
              {/* Status and Actions Bar */}
              <div className="flex items-center justify-between">
                {/* Mobile menu button */}
                <Button
                  onClick={() => setSidebarOpen(!sidebarOpen)}
                  variant="ghost"
                  size="icon"
                  className="sm:hidden mr-2"
                >
                  {sidebarOpen ? <X className="w-4 h-4" /> : <Menu className="w-4 h-4" />}
                </Button>
                {/* Save Status + Ollama indicator */}
                <div className="flex items-center gap-2 sm:gap-4">
                  <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="flex items-center gap-2 text-sm"
                  >
                    {upsert.isPending ? (
                      <div className="flex items-center gap-2 text-muted-foreground">
                        <Loader2 className="w-4 h-4 animate-spin" />
                        <span className="text-sm font-medium">Saving...</span>
                      </div>
                    ) : hasUnsavedChanges ? (
                      <div className="flex items-center gap-2 text-amber-500">
                        <div className="w-2 h-2 bg-amber-500 rounded-full animate-pulse" />
                        <span className="text-sm font-semibold">Unsaved changes</span>
                      </div>
                    ) : selectedId ? (
                      <div className="flex items-center gap-2 text-emerald-500">
                        <Check className="w-4 h-4" />
                        <span className="text-sm font-semibold">All changes saved</span>
                      </div>
                    ) : null}
                  </motion.div>
                  <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 bg-secondary/50 rounded-full backdrop-blur-sm border border-border">
                    <span
                      className={
                        "inline-block h-2 w-2 rounded-full transition-all duration-300 " +
                        (ollamaHealth?.ok ? "bg-emerald-400" : "bg-rose-400 animate-pulse")
                      }
                    />
                    <span
                      className={`text-xs font-medium ${ollamaHealth?.ok ? "text-emerald-400" : "text-rose-400"}`}
                    >
                      {ollamaHealth?.ok ? "Ollama online" : "Ollama offline"}
                    </span>
                  </div>
                </div>

                {/* Action Buttons */}
                <div className="flex items-center gap-1 sm:gap-2">
                  <Button
                    onClick={() => setAvatarOpen(true)}
                    variant="secondary"
                    size="sm"
                    title="Avatar"
                    className="hover:shadow-md transition-all duration-200"
                  >
                    <User className="w-4 h-4" />
                    <span className="hidden lg:inline font-medium">Avatar</span>
                  </Button>
                  <Button
                    onClick={() => setGalleryOpen(true)}
                    variant="secondary"
                    size="sm"
                    title="Gallery"
                    className="hover:shadow-md transition-all duration-200"
                  >
                    <CalendarDays className="w-4 h-4" />
                    <span className="hidden lg:inline font-medium">Gallery</span>
                  </Button>
                  <Button
                    onClick={() => setSettingsOpen(true)}
                    variant="outline"
                    size="sm"
                    title="Settings"
                    className="hover:shadow-md transition-all duration-200"
                  >
                    <Settings className="w-4 h-4" />
                    <span className="hidden lg:inline font-medium">Settings</span>
                  </Button>
                  
                  <Button
                    onClick={handleSave}
                    disabled={!hasUnsavedChanges || upsert.isPending}
                    variant={hasUnsavedChanges ? "primary" : "secondary"}
                    size="sm"
                    title="Save (Cmd/Ctrl + S)"
                    className={`transition-all duration-200 ${hasUnsavedChanges ? 'shadow-lg hover:shadow-xl hover:scale-105' : 'hover:shadow-md'}`}
                  >
                    <Save className="w-4 h-4" />
                    <span className="hidden lg:inline font-semibold">Save</span>
                  </Button>
                  
                  <Button
                    onClick={makeComic}
                    disabled={!selectedId}
                    variant="secondary"
                    size="sm"
                    className="bg-gradient-to-r from-violet-600 via-purple-600 to-indigo-600 hover:from-violet-700 hover:via-purple-700 hover:to-indigo-700 text-white border-0 shadow-lg hover:shadow-xl transition-all duration-200 hover:scale-105 font-semibold"
                    title="Cartoonify"
                  >
                    <Sparkles className="w-4 h-4" />
                    <span className="hidden lg:inline">Cartoonify</span>
                  </Button>
                </div>
              </div>
            </div>
          </div>

          {/* Editor */}
          <div className="flex-1 px-4 sm:px-6 lg:px-8 py-4 sm:py-6 overflow-auto" style={{ maxWidth: '1200px', margin: '0 auto', width: '100%' }}>
            <MarkdownEditor
              value={body}
              onChange={handleBodyChange}
              mode={editorMode}
              onModeChange={setEditorMode}
              placeholder="Start writing your journal entry...

You can use **Markdown** formatting:
- *Italics* and **bold**
- # Headers
- Lists and links
- > Blockquotes

Switch to Preview mode to see your formatted text.

Keyboard shortcuts:
- Cmd/Ctrl + S: Save
- Cmd/Ctrl + N: New entry
- Cmd/Ctrl + E: Toggle preview
- Cmd/Ctrl + K: Focus search"
              className="h-full drop-shadow-sm"
            />
            {/* The inline status blocks were moved into the progress modal for a cleaner UX */}
          </div>
        </motion.div>
      </div>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <GalleryModal
        open={galleryOpen}
        onClose={() => setGalleryOpen(false)}
        onSelect={(entryId) => {
          setGalleryOpen(false);
          loadEntry(entryId);
        }}
      />
      <AvatarModal
        open={avatarOpen}
        onClose={() => setAvatarOpen(false)}
        onSaved={() => {/* no-op for now */}}
      />
      <ComicProgressModal
        open={progressOpen}
        status={comicStatus}
        onClose={() => setProgressOpen(false)}
        onCancel={async () => {
          if (comicJobId) {
            try { await invoke("cancel_job", { jobId: comicJobId }); } catch {}
          }
          setIsPolling(false);
          setProgressOpen(false);
        }}
      />
    </div>
  );
}