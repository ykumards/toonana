import { useEffect, useMemo, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
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

function useInit() {
  const qc = useQueryClient();
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

  const upsert = useMutation({
    mutationFn: async () => {
      const body_cipher = await encrypt(body);
      const payload: EntryUpsert = { id: selectedId ?? undefined, title, body_cipher };
      return invoke<Entry>("db_upsert_entry", { entry: payload });
    },
    onSuccess: async (e) => {
      setSelectedId(e.id);
      await qc.invalidateQueries({ queryKey: ["entries"] });
    },
  });

  const loadEntry = async (id: string) => {
    const e = await invoke<Entry>("db_get_entry", { id });
    setSelectedId(e.id);
    setTitle(e.title);
    try {
      const text = await decrypt(e.body_cipher);
      setBody(text);
    } catch {
      setBody("");
    }
  };

  const startNew = () => {
    setSelectedId(null);
    setTitle("");
    setBody("");
  };

  const makeComic = async () => {
    if (!selectedId) return;
    await invoke<string>("create_comic_job", { entryId: selectedId, style: "nano-banana" });
    // Minimal stub; later show progress
  };

  return (
    <main className="container" style={{ display: "flex", gap: 16 }}>
      <div style={{ width: 280 }}>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <h2 style={{ margin: 0 }}>Entries</h2>
          <button onClick={startNew} style={{ marginLeft: "auto" }}>New</button>
        </div>
        {isLoading ? (
          <p>Loading…</p>
        ) : (
          <ul style={{ padding: 0, listStyle: "none", marginTop: 8 }}>
            {(entries ?? []).map((e) => (
              <li key={e.id}>
                <button onClick={() => loadEntry(e.id)} style={{ width: "100%" }}>{e.title || "(untitled)"}</button>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: 8 }}>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Title"
        />
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          placeholder="Write your entry…"
          rows={16}
        />
        <div style={{ display: "flex", gap: 8 }}>
          <button onClick={() => upsert.mutate()} disabled={upsert.isPending}>
            {upsert.isPending ? "Saving…" : "Save"}
          </button>
          <button onClick={makeComic} disabled={!selectedId}>Make a Comic</button>
        </div>
      </div>
    </main>
  );
}
