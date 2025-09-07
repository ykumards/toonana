# Toonana

Local-first journaling with AI-powered comic generation. Built with Tauri, React, and Rust.

## Overview

Toonana is a personal journaling app with a modern Markdown editor and a playful twist: turn entries into a single-image multi‑panel comic. The app runs as a desktop app via Tauri and stores your data locally.

## Features

- **Markdown Editor**: Rich editor with preview
- **Auto‑Save**: Automatic saving with clear status
- **Keyboard Shortcuts**: Fast workflow (save/new/preview/search)
- **Storyboarding (Ollama)**: Drafts a short 3–4 panel storyboard from your entry
- **Comic Generation (Gemini / Nano‑Banana)**: Renders a single multi‑panel comic image
- **Avatar Cartoonify**: Upload a photo to generate a cartoon avatar
- **Gallery**: Browse generated comics by day
- **Search & Organization**: Quick filter by entry preview text
- **Local‑first Storage**: Data stored on your machine

Note on security: this hackathon build uses a simplified crypto path; encryption APIs are currently stubbed. Do not store highly sensitive information.

## Tech Stack

- **Frontend**: React 19, TypeScript, Vite, TanStack Query, Zustand, Radix UI, Framer Motion
- **Desktop**: Tauri v2
- **Backend**: Rust (Tauri commands)
- **Database**: SQLite (SQLx)
- **Styling**: Tailwind CSS
- **AI**:
  - Storyboarding via Ollama (local LLM)
  - Image generation via Gemini (API key) or Nano‑Banana (optional service)

## Getting Started

### Prerequisites

- Node.js 18+ (Node 20 recommended) and pnpm
- Rust (stable) and platform prerequisites for Tauri
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux: `libgtk-3-dev` and other GTK/WebKit deps
  - Windows: Visual Studio Build Tools (Desktop C++)
  - See Tauri’s platform setup docs for details

### Setup

1. Install dependencies

```bash
pnpm install
```

1. Run the app (Tauri dev)

```bash
pnpm tauri dev
```

1. Build a release bundle

```bash
pnpm tauri build
# Artifacts: src-tauri/target/release/bundle/*
```

Optional: Run frontend only

```bash
pnpm dev
```

Run backend tests

```bash
cd src-tauri && cargo test
```

### Configuration (Settings)

Open Settings in the app to configure:

- **Gemini API Key**: Required for image generation unless using Nano‑Banana.
- **Ollama Base URL**: Default `http://127.0.0.1:11434`.
- **Default Ollama Model**: e.g., `gemma3:1b` (see list via Refresh).
- **Temperature / Top‑P**: Sampling parameters for Ollama prompts.
- **Nano‑Banana**: Optional service for image generation
  - Base URL and optional API key.

Environment variable alternative:

- `GEMINI_API_KEY` can be set in your shell; the app will use it if no key is saved in Settings.

## Architecture

### Frontend

- **`src/App.tsx`**: Main application component managing state and UI
- **`src/components/`**: Reusable UI components
  - `EntriesSidebar`: Navigation and entry list
  - `MarkdownEditor`: Content editing with preview
  - `SettingsModal`: Configure providers and parameters
  - `ComicProgressModal`: Live progress while rendering
  - `GalleryModal`: Browse generated comics by day
  - `AvatarModal`: Cartoonify and set your avatar
- **`src/hooks/`**: Custom React hooks
  - `useAutosave`: Automatic saving functionality
  - `useKeyboardShortcuts`: Global hotkey handling

### Backend

- **`src-tauri/src/lib.rs`**: Tauri commands, job orchestration, logging
- **`src-tauri/src/comic.rs`**: Comic job pipeline (storyboard → render → save)
- **`src-tauri/src/ollama.rs`**: Ollama health, list models, text generation
- **`src-tauri/src/gemini.rs`**: Gemini image generation (streaming + fallback)
- **`src-tauri/src/database.rs`**: SQLite schema and queries
- **`src-tauri/src/utils.rs`**: Data dir and DB path helpers

### Data model (SQLite)

- `entries`: journal entries (body stored as bytes in `body_cipher`)
- `storyboards`: stored storyboard metadata
- `panels`: panel metadata and generated images
- `assets`: future attachments

Security note: encryption is currently stubbed in this build; data is stored locally without full at‑rest encryption.

## Keyboard Shortcuts

- **Cmd/Ctrl + S**: Save current entry
- **Cmd/Ctrl + N**: Create new entry
- **Cmd/Ctrl + E**: Toggle preview mode
- **Cmd/Ctrl + K**: Focus search

## Data Location

Application data is stored in platform-specific directories (Directories crate):

- **macOS**: `~/Library/Application Support/app.toonana.toonana/`
- **Windows**: `%APPDATA%\app\toonana\toonana\`
- **Linux**: `~/.local/share/app/toonana/toonana/`

Generated images are saved under `images/<entry_id>/` inside the data directory. Avatars are stored under `avatars/`.

Logs are written to `logs/toonana.log` in the data directory.

## Submission (Hackathon)

- **Demo Video (≤ 2 minutes)**: Publicly accessible link (no login required)
  - Link: [YouTube Demo](https://youtu.be/va_8D2urgOk)

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the MIT License.

## IDE Setup

- Zed editor (preferred) + Tauri + rust-analyzer (or VS Code + Tauri + rust-analyzer)
