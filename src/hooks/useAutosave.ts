import { useEffect, useRef } from 'react';

interface UseAutosaveOptions {
  onSave: () => void;
  hasChanges: boolean;
  delay?: number; // milliseconds
  enabled?: boolean;
}

export function useAutosave({ 
  onSave, 
  hasChanges, 
  delay = 3000, 
  enabled = true 
}: UseAutosaveOptions) {
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!enabled || !hasChanges) {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
        timeoutRef.current = null;
      }
      return;
    }

    // Clear existing timeout
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    // Set new timeout
    timeoutRef.current = setTimeout(() => {
      onSave();
      timeoutRef.current = null;
    }, delay);

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, [hasChanges, delay, enabled, onSave]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);
}