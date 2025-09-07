# Toonana

A secure, encrypted desktop journaling application built with Tauri, React, and Rust.

## Overview

Toonana is a personal journaling app that prioritizes privacy and security through client-side encryption. Write your thoughts in markdown, organize entries, and optionally generate comic visualizations of your journal entries.

## Features

- **🔒 End-to-End Encryption**: All journal content encrypted with AES-256-GCM
- **📝 Markdown Editor**: Rich text editing with live preview
- **💾 Auto-Save**: Automatic saving with visual feedback
- **⌨️ Keyboard Shortcuts**: Efficient workflow with hotkeys
- **🎨 Comic Generation**: Transform entries into comic panels (coming soon)
- **🔍 Search & Organization**: Find entries by title and metadata
- **🛡️ Secure Storage**: Encryption keys stored in system keyring

## Tech Stack

- **Frontend**: React 19, TypeScript, Vite, TanStack Query, Zustand
- **Backend**: Rust with Tauri v2
- **Database**: SQLite with SQLx
- **Encryption**: AES-GCM with system keyring integration
- **Styling**: Tailwind CSS with custom design system

## Development

### Prerequisites

- Node.js (18+) and pnpm
- Rust (latest stable)
- Platform-specific Tauri dependencies

### Setup

1. Clone the repository
2. Install frontend dependencies:
   ```bash
   pnpm install
   ```
3. Install Rust dependencies:
   ```bash
   cd src-tauri && cargo build
   ```

### Commands

```bash
# Development (frontend only)
pnpm dev

# Development (full Tauri app)
pnpm tauri dev

# Build frontend with type checking
pnpm build

# Build complete application
pnpm tauri build

# Run backend tests
cd src-tauri && cargo test
```

### Architecture

#### Frontend Structure
- **`src/App.tsx`**: Main application component managing state and UI
- **`src/components/`**: Reusable UI components
  - `EntriesSidebar`: Navigation and entry list
  - `MarkdownEditor`: Content editing with preview
- **`src/hooks/`**: Custom React hooks
  - `useAutosave`: Automatic saving functionality
  - `useKeyboardShortcuts`: Global hotkey handling

#### Backend Structure
- **`src-tauri/src/lib.rs`**: Core application logic and Tauri commands
- **Database Schema**:
  - `entries`: Main journal entries with encrypted content
  - `storyboards`: Comic generation metadata
  - `panels`: Individual comic panels
  - `assets`: File attachments and images

#### Security Model
1. **Key Generation**: 256-bit keys generated on first run
2. **Key Storage**: Keys stored securely in system keyring
3. **Field Encryption**: Only sensitive content (entry body) is encrypted
4. **Local Storage**: All data stored locally in SQLite database

### Keyboard Shortcuts

- **Cmd/Ctrl + S**: Save current entry
- **Cmd/Ctrl + N**: Create new entry
- **Cmd/Ctrl + E**: Toggle preview mode
- **Cmd/Ctrl + K**: Focus search

### Data Location

Application data is stored in platform-specific directories:
- **macOS**: `~/Library/Application Support/toonana/toonana/`
- **Windows**: `%APPDATA%\toonana\toonana\`
- **Linux**: `~/.local/share/toonana/toonana/`

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the MIT License.

## IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)