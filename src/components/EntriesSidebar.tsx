import { useState, useMemo } from "react";
import { format, formatDistance } from "date-fns";
import { Search, Plus, BookOpen, Calendar } from "lucide-react";
import clsx from "clsx";

type EntryListItem = {
  id: string;
  created_at: string;
  updated_at: string;
  title: string;
  mood?: string | null;
  tags?: unknown | null;
};

interface EntriesSidebarProps {
  entries: EntryListItem[];
  selectedId: string | null;
  isLoading: boolean;
  onEntrySelect: (id: string) => void;
  onNewEntry: () => void;
  searchInputRef?: React.RefObject<HTMLInputElement | null> | React.MutableRefObject<HTMLInputElement | null>;
}

export function EntriesSidebar({ 
  entries, 
  selectedId, 
  isLoading, 
  onEntrySelect, 
  onNewEntry,
  searchInputRef 
}: EntriesSidebarProps) {
  const [searchQuery, setSearchQuery] = useState("");

  const filteredEntries = useMemo(() => {
    if (!searchQuery.trim()) return entries;
    
    const query = searchQuery.toLowerCase();
    return entries.filter(entry => 
      entry.title.toLowerCase().includes(query)
    );
  }, [entries, searchQuery]);

  const groupedEntries = useMemo(() => {
    const groups: { [key: string]: EntryListItem[] } = {};
    
    filteredEntries.forEach(entry => {
      const date = new Date(entry.created_at);
      const today = new Date();
      const yesterday = new Date(today);
      yesterday.setDate(yesterday.getDate() - 1);
      
      let groupKey: string;
      
      if (format(date, 'yyyy-MM-dd') === format(today, 'yyyy-MM-dd')) {
        groupKey = 'Today';
      } else if (format(date, 'yyyy-MM-dd') === format(yesterday, 'yyyy-MM-dd')) {
        groupKey = 'Yesterday';
      } else {
        groupKey = format(date, 'MMMM yyyy');
      }
      
      if (!groups[groupKey]) {
        groups[groupKey] = [];
      }
      groups[groupKey].push(entry);
    });

    // Sort entries within each group by date (newest first)
    Object.keys(groups).forEach(key => {
      groups[key].sort((a, b) => 
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      );
    });

    return groups;
  }, [filteredEntries]);

  const formatEntryDate = (dateString: string) => {
    const date = new Date(dateString);
    const now = new Date();
    const diffInHours = (now.getTime() - date.getTime()) / (1000 * 60 * 60);
    
    if (diffInHours < 24) {
      return format(date, 'h:mm a');
    } else if (diffInHours < 168) { // Less than a week
      return formatDistance(date, now, { addSuffix: true });
    } else {
      return format(date, 'MMM d');
    }
  };

  const getMoodEmoji = (mood: string | null | undefined) => {
    if (!mood) return null;
    
    const moodMap: { [key: string]: string } = {
      'happy': '😊',
      'sad': '😢',
      'excited': '🎉',
      'anxious': '😰',
      'calm': '😌',
      'frustrated': '😤',
      'grateful': '🙏',
      'thoughtful': '🤔',
    };
    
    return moodMap[mood.toLowerCase()] || '💭';
  };

  return (
    <div className="w-80 h-full bg-surface-primary border-r border-journal-200 flex flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-4 border-b border-journal-100 bg-surface-secondary">
        <div className="flex items-center gap-2">
          <BookOpen size={20} className="text-text-tertiary" />
          <h2 className="text-lg font-semibold text-text-primary m-0">Journal</h2>
        </div>
        
        <button 
          onClick={onNewEntry}
          className="flex items-center justify-center w-9 h-9 bg-accent-500 text-white rounded-md hover:bg-accent-600 hover:-translate-y-0.5 hover:shadow-journal-lg transition-all duration-150 focus-ring"
          aria-label="New entry"
        >
          <Plus size={16} />
        </button>
      </div>

      {/* Search */}
      <div className="px-5 py-4 border-b border-journal-100">
        <div className="relative flex items-center">
          <Search size={16} className="absolute left-3 text-text-muted z-10" />
          <input
            ref={searchInputRef}
            type="text"
            placeholder="Search entries... (Cmd+K)"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-10 pr-3 py-2.5 border border-journal-300 rounded-md text-sm bg-surface-primary text-text-secondary transition-all duration-150 focus-ring placeholder:text-text-muted"
          />
        </div>
      </div>

      {/* Entries List */}
      <div className="flex-1 overflow-y-auto">
        {isLoading ? (
          <div className="flex flex-col items-center justify-center h-48 gap-3 text-text-tertiary">
            <div className="w-6 h-6 border-2 border-journal-300 border-t-accent-500 rounded-full animate-spin"></div>
            <p>Loading entries...</p>
          </div>
        ) : filteredEntries.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-72 px-5 text-center text-text-tertiary gap-4">
            {searchQuery ? (
              <>
                <Search size={48} className="text-journal-300" />
                <p className="text-base font-medium">No entries found for "{searchQuery}"</p>
                <button 
                  onClick={() => setSearchQuery("")} 
                  className="px-4 py-2 border border-journal-300 bg-surface-primary text-text-secondary rounded-md text-sm font-medium hover:bg-surface-secondary hover:border-journal-400 transition-all duration-150 focus-ring"
                >
                  Clear search
                </button>
              </>
            ) : (
              <>
                <BookOpen size={48} className="text-journal-300" />
                <div>
                  <p className="text-base font-medium">No entries yet</p>
                  <p className="text-sm text-text-muted mt-1">Start writing your first journal entry</p>
                </div>
                <button 
                  onClick={onNewEntry} 
                  className="flex items-center gap-2 px-4 py-2 bg-accent-500 text-white border border-accent-500 rounded-md text-sm font-medium hover:bg-accent-600 hover:border-accent-600 transition-all duration-150 focus-ring"
                >
                  <Plus size={16} />
                  Start Writing
                </button>
              </>
            )}
          </div>
        ) : (
          Object.entries(groupedEntries).map(([groupName, groupEntries]) => (
            <div key={groupName} className="mb-6">
              <div className="sticky top-0 z-10 flex items-center gap-2 px-5 py-3 text-xs font-semibold text-text-tertiary uppercase tracking-wide border-b border-journal-50 bg-surface-secondary">
                <Calendar size={14} />
                <span className="flex-1">{groupName}</span>
                <span className="bg-journal-200 text-text-tertiary px-1.5 py-0.5 rounded-full text-xs font-medium min-w-[20px] text-center">
                  {groupEntries.length}
                </span>
              </div>
              
              <div className="bg-surface-primary">
                {groupEntries.map(entry => (
                  <button
                    key={entry.id}
                    onClick={() => onEntrySelect(entry.id)}
                    className={clsx(
                      "w-full px-5 py-4 border-b border-journal-50 text-left transition-all duration-150 relative",
                      selectedId === entry.id
                        ? "bg-accent-50 border-l-3 border-l-accent-500"
                        : "hover:bg-surface-secondary"
                    )}
                  >
                    {selectedId === entry.id && (
                      <div className="absolute top-0 right-0 bottom-0 w-0.5 bg-accent-500"></div>
                    )}
                    
                    <div className="flex flex-col gap-2">
                      <div className="flex items-start justify-between gap-2">
                        <h3 className="text-sm font-medium text-text-primary line-clamp-1 flex-1 overflow-hidden text-ellipsis whitespace-nowrap m-0">
                          {entry.title || "Untitled"}
                        </h3>
                        {getMoodEmoji(entry.mood) && (
                          <span className="text-base flex-shrink-0">
                            {getMoodEmoji(entry.mood)}
                          </span>
                        )}
                      </div>
                      
                      <div className="flex items-center gap-3">
                        <span className="text-xs text-text-tertiary font-normal">
                          {formatEntryDate(entry.created_at)}
                        </span>
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}