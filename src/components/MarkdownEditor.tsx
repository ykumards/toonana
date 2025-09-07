import { useState, useCallback, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Eye, Edit3 } from "lucide-react";
import { cn } from "@/lib/utils";
import { motion, AnimatePresence } from "framer-motion";

interface MarkdownEditorProps {
  value: string;
  onChange: (value: string) => void;
  mode?: "edit" | "preview";
  onModeChange?: (mode: "edit" | "preview") => void;
  placeholder?: string;
  className?: string;
}

export function MarkdownEditor({ 
  value, 
  onChange, 
  mode: controlledMode, 
  onModeChange, 
  placeholder = "Start writing...", 
  className 
}: MarkdownEditorProps) {
  const [internalMode, setInternalMode] = useState<"edit" | "preview">("edit");
  const [focused, setFocused] = useState(false);
  
  const mode = controlledMode ?? internalMode;
  const setMode = onModeChange ?? setInternalMode;

  const handleTextareaChange = useCallback(
    (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      onChange(e.target.value);
    },
    [onChange]
  );

  const markdownComponents = useMemo(() => ({
    h1: ({ children }: any) => (
      <h1 className="text-journal-title text-text-primary mb-6 pb-3 border-b-2 border-journal-200">
        {children}
      </h1>
    ),
    h2: ({ children }: any) => (
      <h2 className="text-journal-heading text-text-primary mb-4 mt-8 pb-2 border-b border-journal-100">
        {children}
      </h2>
    ),
    h3: ({ children }: any) => (
      <h3 className="text-journal-subheading text-text-secondary mb-3 mt-6">
        {children}
      </h3>
    ),
    h4: ({ children }: any) => (
      <h4 className="text-xl font-semibold text-text-secondary mb-3 mt-5">
        {children}
      </h4>
    ),
    p: ({ children }: any) => (
      <p className="text-journal-body text-text-secondary mb-4 leading-relaxed">
        {children}
      </p>
    ),
    ul: ({ children }: any) => (
      <ul className="mb-4 pl-6 space-y-1 list-disc marker:text-journal-400">
        {children}
      </ul>
    ),
    ol: ({ children }: any) => (
      <ol className="mb-4 pl-6 space-y-1 list-decimal marker:text-journal-400">
        {children}
      </ol>
    ),
    li: ({ children }: any) => (
      <li className="text-journal-body text-text-secondary">
        {children}
      </li>
    ),
    code: ({ children, inline }: any) => (
      inline ? (
        <code className="bg-journal-100 text-red-600 px-1.5 py-0.5 rounded-sm font-mono text-sm">
          {children}
        </code>
      ) : (
        <code className="font-mono text-sm text-text-secondary leading-relaxed">
          {children}
        </code>
      )
    ),
    pre: ({ children }: any) => (
      <pre className="bg-surface-tertiary border border-journal-200 rounded-journal p-4 mb-6 overflow-x-auto">
        {children}
      </pre>
    ),
    blockquote: ({ children }: any) => (
      <blockquote className="border-l-4 border-journal-300 pl-4 mb-6 text-text-tertiary italic">
        {children}
      </blockquote>
    ),
    strong: ({ children }: any) => (
      <strong className="font-semibold text-text-primary">
        {children}
      </strong>
    ),
    em: ({ children }: any) => (
      <em className="italic text-text-secondary">
        {children}
      </em>
    ),
    a: ({ children, href }: any) => (
      <a 
        href={href} 
        className="text-accent-600 hover:text-accent-700 border-b border-accent-300 hover:border-accent-600 transition-colors duration-150" 
        target="_blank" 
        rel="noopener noreferrer"
      >
        {children}
      </a>
    ),
  }), []);

  return (
    <div className={cn("flex flex-col h-full bg-white rounded-xl border border-slate-200 overflow-hidden shadow-sm", className)}>
      {/* Modern Mode Toggle */}
      <div className="flex items-center justify-between px-3 sm:px-6 py-2 sm:py-3 border-b border-slate-200 bg-gradient-to-b from-slate-50 to-white">
        <div className="relative flex bg-slate-100 rounded-lg p-1">
          <div
            className={cn(
              "absolute top-1 h-8 bg-white rounded-md shadow-sm transition-all duration-200",
              mode === "edit" ? "left-1 w-[72px]" : "left-[80px] w-[88px]"
            )}
          />
          <button
            onClick={() => setMode("edit")}
            className={cn(
              "relative z-10 flex items-center gap-1 sm:gap-2 px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm font-medium transition-colors duration-200",
              mode === "edit" ? "text-slate-900" : "text-slate-500 hover:text-slate-700"
            )}
          >
            <Edit3 size={14} />
            Write
          </button>
          <button
            onClick={() => setMode("preview")}
            className={cn(
              "relative z-10 flex items-center gap-1 sm:gap-2 px-2 sm:px-3 py-1 sm:py-1.5 text-xs sm:text-sm font-medium transition-colors duration-200",
              mode === "preview" ? "text-slate-900" : "text-slate-500 hover:text-slate-700"
            )}
          >
            <Eye size={14} />
            Preview
          </button>
        </div>
      </div>

      {/* Editor Content with Animation */}
      <div className="flex flex-1 min-h-0 relative">
        <AnimatePresence mode="wait">
          {mode === "edit" ? (
            <motion.div
              key="editor"
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -10 }}
              transition={{ duration: 0.15 }}
              className="flex-1 flex"
            >
              <textarea
                value={value}
                onChange={handleTextareaChange}
                onFocus={() => setFocused(true)}
                onBlur={() => setFocused(false)}
                placeholder={placeholder}
                className={cn(
                  "flex-1 p-3 sm:p-4 lg:p-6 font-sans text-sm sm:text-base leading-relaxed text-slate-900 bg-transparent resize-none overflow-y-auto",
                  "placeholder:text-slate-400 placeholder:leading-relaxed",
                  "focus:outline-none focus:bg-gradient-to-b focus:from-blue-50/50 focus:to-transparent",
                  "transition-colors duration-200"
                )}
                spellCheck="true"
              />
            </motion.div>
          ) : (
            <motion.div
              key="preview"
              initial={{ opacity: 0, x: 10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 10 }}
              transition={{ duration: 0.15 }}
              className="flex-1 p-3 sm:p-4 lg:p-6 overflow-y-auto"
            >
              {value.trim() ? (
                <div className="prose prose-slate prose-lg max-w-none">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    components={markdownComponents}
                  >
                    {value}
                  </ReactMarkdown>
                </div>
              ) : (
                <div className="flex items-center justify-center h-full">
                  <p className="text-slate-400 italic">Nothing to preview yet. Switch to Write mode to start writing.</p>
                </div>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}