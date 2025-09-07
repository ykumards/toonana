import { useEffect, useState, useRef } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { Save, Sparkles } from "lucide-react";
import { Settings } from "lucide-react";
import clsx from "clsx";
import { EntriesSidebar } from "./components/EntriesSidebar";
import { MarkdownEditor } from "./components/MarkdownEditor";
import { SettingsModal } from "./components/SettingsModal";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useAutosave } from "./hooks/useAutosave";
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
    <div className="journal-container">
      <div className="journal-layout">
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
        <div className="flex-1 flex flex-col bg-surface-secondary">
          {/* Editor Header */}
          <div className="flex items-center justify-between px-journal py-4 border-b border-journal-200 bg-surface-primary">
            <div className="flex-1 max-w-2xl">
              <input
                value={title}
                onChange={(e) => handleTitleChange(e.target.value)}
                onBlur={handleSave}
                placeholder="Entry title..."
                className="w-full text-2xl font-semibold text-text-primary bg-transparent border-none outline-none placeholder:text-text-muted focus:placeholder:text-text-tertiary transition-colors duration-150"
              />
            </div>
            
            <div className="flex items-center gap-3">
              {/* Save Status */}
              <div className="flex items-center gap-2 text-sm">
                {upsert.isPending ? (
                  <div className="flex items-center gap-2 text-text-tertiary">
                    <div className="w-4 h-4 border-2 border-journal-300 border-t-accent-500 rounded-full animate-spin"></div>
                    <span>Saving...</span>
                  </div>
                ) : hasUnsavedChanges ? (
                  <span className="text-amber-600 font-medium">Unsaved changes</span>
                ) : selectedId ? (
                  <span className="text-green-600 font-medium">Saved</span>
                ) : null}
              </div>

              {/* Action Buttons */}
              <div className="flex gap-2">
                <button
                  onClick={() => setSettingsOpen(true)}
                  className="flex items-center gap-2 px-3 py-2 border rounded-md text-sm font-medium transition-all duration-150 focus-ring bg-journal-50 text-text-primary border-journal-200 hover:bg-journal-100 hover:border-journal-300"
                >
                  <Settings size={16} />
                  Settings
                </button>
                <button
                  onClick={handleSave}
                  disabled={!hasUnsavedChanges || upsert.isPending}
                  className={clsx(
                    "flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium transition-all duration-150 focus-ring",
                    hasUnsavedChanges
                      ? "bg-accent-500 text-white hover:bg-accent-600"
                      : "bg-journal-100 text-text-muted cursor-not-allowed"
                  )}
                >
                  <Save size={16} />
                  Save
                </button>
                
                <button
                  onClick={makeComic}
                  disabled={!selectedId}
                  className="flex items-center gap-2 px-3 py-2 bg-purple-100 text-purple-700 border border-purple-200 rounded-md text-sm font-medium hover:bg-purple-200 hover:border-purple-300 disabled:opacity-50 disabled:cursor-not-allowed transition-all duration-150 focus-ring"
                >
                  <Sparkles size={16} />
                  Make Comic
                </button>
              </div>
            </div>
          </div>

          {/* Editor */}
          <div className="flex-1 p-journal">
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
        </div>
      </div>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  );
}