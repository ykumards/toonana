# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Toonana is a Tauri-based desktop application built with React, TypeScript, and Rust. It appears to be a personal journaling/notes application with encryption capabilities.

## Tech Stack

- **Frontend**: React 19, TypeScript, Vite, TanStack Query, Zustand
- **Backend**: Rust with Tauri v2
- **Database**: SQLite with SQLx
- **Encryption**: AES-GCM for field encryption, keyring for secure key storage

## Commands

### Development
- `pnpm dev` - Start Vite development server (frontend only)
- `pnpm tauri dev` - Start Tauri development environment (full app)

### Build
- `pnpm build` - Build frontend with TypeScript check and Vite
- `pnpm tauri build` - Build complete Tauri application

### Testing
- `cd src-tauri && cargo test` - Run Rust backend tests

## Architecture

### Frontend Structure
- Main entry point: `src/main.tsx` with React Query provider
- State management with Zustand
- API communication through Tauri's IPC system

### Backend Structure
- Core logic in `src-tauri/src/lib.rs`
- Key features:
  - Encrypted journal entries with AES-256-GCM
  - SQLite database stored in platform-specific app data directory
  - Secure key storage using system keyring
  - Background job management with DashMap
  - Entry CRUD operations with encryption/decryption

### Database Schema
The app uses SQLite with entries containing:
- ID (UUID)
- Created/updated timestamps
- Title (plaintext)
- Body (encrypted)
- Mood and tags (JSON)
- Embeddings support

## Development Notes

- Vite dev server runs on port 1420
- Tauri expects strict port binding
- Frontend/backend separation with IPC bridge
- Uses pnpm as package manager