import { useState, useMemo } from "react";
import { format, formatDistance } from "date-fns";
import { Search, Plus, BookOpen, Calendar, FileText, Clock } from "lucide-react";
import { cn } from "@/lib/utils";
import { ScrollArea } from "./ui/scroll-area";
import { Button } from "./ui/button";
import { motion, AnimatePresence } from "framer-motion";

type EntryListItem = {
  id: string;
  created_at: string;
  updated_at: string;
  body_preview?: string | null;
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
      (entry.body_preview || "").toLowerCase().includes(query)
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
    <motion.div 
      initial={{ x: -20, opacity: 0 }}
      animate={{ x: 0, opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="w-full sm:w-72 lg:w-80 bg-gradient-to-b from-slate-50 to-slate-100 border-r border-slate-200 flex flex-col h-full absolute sm:relative z-20 sm:z-auto"
    >
      {/* Modern Header */}
      <div className="p-3 sm:p-4 border-b border-slate-200 bg-white/80 backdrop-blur-sm">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-2">
            <div className="p-1.5 sm:p-2 bg-gradient-to-br from-blue-500 to-blue-600 rounded-lg text-white shadow-lg">
              <BookOpen className="w-4 h-4 sm:w-5 sm:h-5" />
            </div>
            <div>
              <h1 className="text-base sm:text-lg font-bold text-slate-900">Journal</h1>
              <p className="text-xs text-slate-500 hidden sm:block">{entries?.length || 0} entries</p>
            </div>
          </div>
          <Button
            onClick={onNewEntry}
            size="icon"
            variant="primary"
            className="rounded-full shadow-lg"
            title="New Entry (Cmd/Ctrl + N)"
          >
            <Plus size={18} />
          </Button>
        </div>

        {/* Modern Search Bar */}
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400" size={16} />
          <input
            ref={searchInputRef}
            type="text"
            placeholder="Search entries... (⌘K)"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full pl-9 sm:pl-10 pr-3 py-1.5 sm:py-2 bg-slate-50 border border-slate-200 rounded-lg text-xs sm:text-sm placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all"
          />
        </div>
      </div>

      {/* Entries List with ScrollArea */}
      <ScrollArea className="flex-1">
        <div className="p-3 sm:p-4">
          {isLoading ? (
            <motion.div 
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="flex flex-col items-center justify-center py-12 text-slate-400"
            >
              <div className="p-3 bg-slate-100 rounded-full mb-3">
                <Clock className="animate-spin" size={24} />
              </div>
              <p className="text-sm font-medium">Loading entries...</p>
            </motion.div>
          ) : filteredEntries.length === 0 ? (
            <motion.div 
              initial={{ opacity: 0, y: 20 }}
              animate={{ opacity: 1, y: 0 }}
              className="flex flex-col items-center justify-center py-12 text-slate-400"
            >
              {searchQuery ? (
                <>
                  <div className="p-4 bg-gradient-to-br from-slate-100 to-slate-200 rounded-full mb-4">
                    <Search size={32} />
                  </div>
                  <p className="text-sm font-medium text-slate-600 mb-1">No entries found for "{searchQuery}"</p>
                  <Button
                    onClick={() => setSearchQuery("")}
                    variant="outline"
                    size="sm"
                  >
                    Clear search
                  </Button>
                </>
              ) : (
                <>
                  <div className="p-4 bg-gradient-to-br from-slate-100 to-slate-200 rounded-full mb-4">
                    <FileText size={32} />
                  </div>
                  <p className="text-sm font-medium text-slate-600 mb-1">No entries yet</p>
                  <p className="text-xs text-slate-400 mb-4">Start writing your first journal entry</p>
                  <Button
                    onClick={onNewEntry}
                    variant="primary"
                    size="sm"
                  >
                    <Plus size={16} />
                    Start Writing
                  </Button>
                </>
              )}
            </motion.div>
          ) : (
            <div className="space-y-6">
              <AnimatePresence>
                {Object.entries(groupedEntries).map(([groupName, groupEntries], index) => (
                  <motion.div
                    key={groupName}
                    initial={{ opacity: 0, y: 20 }}
                    animate={{ opacity: 1, y: 0 }}
                    transition={{ delay: index * 0.05 }}
                    className="space-y-2"
                  >
                    {/* Date Group Header */}
                    <div className="flex items-center gap-2 px-2 py-1">
                      <Calendar className="text-slate-400" size={14} />
                      <span className="text-xs font-semibold text-slate-500 uppercase tracking-wider">{groupName}</span>
                      <span className="ml-auto text-xs px-2 py-0.5 bg-slate-200 text-slate-600 rounded-full">
                        {groupEntries.length}
                      </span>
                    </div>
                    
                    {/* Entries */}
                    {groupEntries.map((entry) => (
                      <motion.div
                        key={entry.id}
                        whileHover={{ scale: 1.02 }}
                        whileTap={{ scale: 0.98 }}
                        onClick={() => onEntrySelect(entry.id)}
                        className={cn(
                          "relative p-2 sm:p-3 rounded-lg cursor-pointer transition-all",
                          "hover:shadow-md hover:bg-white",
                          entry.id === selectedId
                            ? "bg-gradient-to-r from-blue-50 to-blue-100 border border-blue-200 shadow-md"
                            : "bg-white border border-slate-200"
                        )}
                      >
                        {entry.id === selectedId && (
                          <motion.div
                            layoutId="selectedIndicator"
                            className="absolute left-0 top-1/2 -translate-y-1/2 w-1 h-8 bg-blue-500 rounded-r-full"
                          />
                        )}
                        <div className="flex items-start justify-between">
                          <div className="flex-1 min-w-0">
                            <p className="text-xs text-slate-500 mb-1">
                              <Clock className="inline mr-1 w-2.5 h-2.5" />
                              {formatEntryDate(entry.created_at)}
                            </p>
                            <p className={cn(
                              "text-xs sm:text-sm truncate",
                              entry.id === selectedId ? "text-blue-900" : "text-slate-700"
                            )}>
                              {entry.body_preview || "Empty entry"}
                            </p>
                          </div>
                          {getMoodEmoji(entry.mood) && (
                            <span className="text-lg">{getMoodEmoji(entry.mood)}</span>
                          )}
                        </div>
                      </motion.div>
                    ))}
                  </motion.div>
                ))}
              </AnimatePresence>
            </div>
          )}
        </div>
      </ScrollArea>
    </motion.div>
  );
}