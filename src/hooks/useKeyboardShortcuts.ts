import { useEffect } from 'react';

interface KeyboardShortcuts {
  onSave?: () => void;
  onNewEntry?: () => void;
  onTogglePreview?: () => void;
  onSearch?: () => void;
}

export function useKeyboardShortcuts({
  onSave,
  onNewEntry,
  onTogglePreview,
  onSearch
}: KeyboardShortcuts) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0;
      const modifierKey = isMac ? e.metaKey : e.ctrlKey;

      // Cmd/Ctrl + S to save
      if (modifierKey && e.key === 's') {
        e.preventDefault();
        onSave?.();
        return;
      }

      // Cmd/Ctrl + N for new entry
      if (modifierKey && e.key === 'n') {
        e.preventDefault();
        onNewEntry?.();
        return;
      }

      // Cmd/Ctrl + E to toggle preview
      if (modifierKey && e.key === 'e') {
        e.preventDefault();
        onTogglePreview?.();
        return;
      }

      // Cmd/Ctrl + K for search (focus search input)
      if (modifierKey && e.key === 'k') {
        e.preventDefault();
        onSearch?.();
        return;
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, [onSave, onNewEntry, onTogglePreview, onSearch]);
}