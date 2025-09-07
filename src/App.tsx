import { useEffect, useState, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { Save, Sparkles, Settings, Check, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { EntriesSidebar } from "./components/EntriesSidebar";
import { MarkdownEditor } from "./components/MarkdownEditor";
import { SettingsModal } from "./components/SettingsModal";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useAutosave } from "./hooks/useAutosave";
import { Button } from "./components/ui/button";
import { motion } from "framer-motion";
import "./App.css";

type Entry = {
  id: string;
  created_at: string;
  updated_at: string;
  title: string;
  body_cipher: number[];
  mood?: string | null;
  tags?: unknown | null;
};

type EntryUpsert = {
  id?: string;
  title: string;
  body_cipher: number[];
  mood?: string | null;
  tags?: unknown | null;
};

type EntryListItem = {
  id: string;
  created_at: string;
  updated_at: string;
  title: string;
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
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  const [editorMode, setEditorMode] = useState<"edit" | "preview">("edit");
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [comicJobId, setComicJobId] = useState<string | null>(null);
  const [comicStatus, setComicStatus] = useState<ComicJobStatus | null>(null);
  const [isPolling, setIsPolling] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  const upsert = useMutation({
    mutationFn: async () => {
      const body_cipher = await encrypt(body);
      const payload: EntryUpsert = { id: selectedId ?? undefined, title, body_cipher };
      return invoke<Entry>("db_upsert_entry", { entry: payload });
    },
    onSuccess: async (e) => {
      setSelectedId(e.id);
      setHasUnsavedChanges(false);
      await qc.invalidateQueries({ queryKey: ["entries"] });
    },
  });

  const loadEntry = async (id: string) => {
    // Save current entry if there are unsaved changes
    if (hasUnsavedChanges) {
      await upsert.mutateAsync();
    }

    const e = await invoke<Entry>("db_get_entry", { id });
    setSelectedId(e.id);
    setTitle(e.title);
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
    setTitle("");
    setBody("");
    setHasUnsavedChanges(false);
  };

  const makeComic = async () => {
    if (!selectedId) return;
    const job = await invoke<string>("create_comic_job", { entry_id: selectedId, style: "nano-banana" });
    setComicJobId(job);
    setIsPolling(true);
  };

  useEffect(() => {
    if (!comicJobId || !isPolling) return;
    let stopped = false;
    const poll = async () => {
      try {
        const status = await invoke<ComicJobStatus>("get_comic_job_status", { job_id: comicJobId });
        if (stopped) return;
        setComicStatus(status);
        const stage = status.stage as ComicStage;
        if (stage.stage === "done" || stage.stage === "failed") {
          setIsPolling(false);
          return;
        }
      } catch (e) {
        // stop polling on error
        setIsPolling(false);
        return;
      }
      setTimeout(poll, 400);
    };
    poll();
    return () => { stopped = true; };
  }, [comicJobId, isPolling]);

  

  // Track changes
  const handleTitleChange = (newTitle: string) => {
    setTitle(newTitle);
    setHasUnsavedChanges(true);
  };

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
    <div className="h-screen w-full bg-slate-50 overflow-hidden">
      <div className="flex h-full">
        {/* Sidebar */}
        <EntriesSidebar
          entries={entries ?? []}
          selectedId={selectedId}
          isLoading={isLoading}
          onEntrySelect={loadEntry}
          onNewEntry={startNew}
          searchInputRef={searchInputRef}
        />

        {/* Main Editor */}
        <motion.div 
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.1 }}
          className="flex-1 flex flex-col bg-white"
        >
          {/* Modern Editor Header */}
          <div className="border-b border-slate-200 bg-gradient-to-b from-white to-slate-50">
            <div className="px-8 py-6">
              <input
                value={title}
                onChange={(e) => handleTitleChange(e.target.value)}
                onBlur={handleSave}
                placeholder="Entry title..."
                className="w-full text-3xl font-bold text-slate-900 bg-transparent border-none outline-none placeholder:text-slate-400 focus:placeholder:text-slate-500 transition-colors duration-150"
              />
              
              {/* Status and Actions Bar */}
              <div className="flex items-center justify-between mt-4">
                {/* Save Status */}
                <div className="flex items-center gap-2">
                  <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="flex items-center gap-2 text-sm"
                  >
                    {upsert.isPending ? (
                      <div className="flex items-center gap-2 text-slate-500">
                        <Loader2 className="w-4 h-4 animate-spin" />
                        <span>Saving...</span>
                      </div>
                    ) : hasUnsavedChanges ? (
                      <div className="flex items-center gap-2 text-amber-600">
                        <div className="w-2 h-2 bg-amber-600 rounded-full animate-pulse" />
                        <span className="font-medium">Unsaved changes</span>
                      </div>
                    ) : selectedId ? (
                      <div className="flex items-center gap-2 text-green-600">
                        <Check className="w-4 h-4" />
                        <span className="font-medium">All changes saved</span>
                      </div>
                    ) : null}
                  </motion.div>
                </div>

                {/* Action Buttons */}
                <div className="flex items-center gap-2">
                  <Button
                    onClick={() => setSettingsOpen(true)}
                    variant="outline"
                    size="sm"
                  >
                    <Settings className="w-4 h-4" />
                    Settings
                  </Button>
                  
                  <Button
                    onClick={handleSave}
                    disabled={!hasUnsavedChanges || upsert.isPending}
                    variant={hasUnsavedChanges ? "primary" : "secondary"}
                    size="sm"
                  >
                    <Save className="w-4 h-4" />
                    Save
                  </Button>
                  
                  <Button
                    onClick={makeComic}
                    disabled={!selectedId}
                    variant="secondary"
                    size="sm"
                    className="bg-gradient-to-r from-purple-500 to-pink-500 hover:from-purple-600 hover:to-pink-600 text-white border-0"
                  >
                    <Sparkles className="w-4 h-4" />
                    Make Comic
                  </Button>
                </div>
              </div>
            </div>
          </div>

          {/* Editor */}
          <div className="flex-1 px-8 py-6 overflow-auto">
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
              className="h-full"
            />
            {comicStatus ? (
              <div className="mt-3 text-sm text-text-tertiary">
                {(() => {
                  const s = comicStatus.stage as ComicStage;
                  if (s.stage === "rendering") {
                    return `Rendering panels ${s.completed}/${s.total}...`;
                  }
                  if (s.stage === "failed") {
                    return `Failed: ${s.error}`;
                  }
                  if (s.stage === "done") {
                    return "Comic ready.";
                  }
                  return `Stage: ${s.stage}`;
                })()}
              </div>
            ) : null}

            {comicStatus?.storyboard_text ? (
              <div className="mt-3 p-3 border border-journal-200 rounded-md bg-journal-50">
                <div className="text-sm font-medium mb-1 text-text-primary">Storyboard</div>
                <pre className="whitespace-pre-wrap text-sm text-text-secondary">{comicStatus.storyboard_text}</pre>
              </div>
            ) : null}

            {comicStatus?.result_image_path ? (
              <div className="mt-3">
                <div className="text-sm font-medium mb-1 text-text-primary">Generated Image</div>
                <img
                  src={convertFileSrc(comicStatus.result_image_path)}
                  alt="Generated comic"
                  className="max-w-full rounded-md border border-journal-200"
                />
              </div>
            ) : null}
          </div>
        </motion.div>
      </div>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}