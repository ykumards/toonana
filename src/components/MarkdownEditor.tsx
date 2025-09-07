import { useState, useCallback, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Eye, Edit3 } from "lucide-react";
import clsx from "clsx";

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
    <div className={clsx("flex flex-col h-full bg-surface-primary rounded-journal overflow-hidden shadow-journal", className)}>
      {/* Mode Toggle */}
      <div className="flex items-center justify-between px-journal py-3 border-b border-journal-200 bg-surface-secondary min-h-[48px]">
        <div className="flex gap-0.5 bg-journal-200 rounded-md p-0.5">
          <button
            onClick={() => setMode("edit")}
            className={clsx(
              "flex items-center gap-2 px-3 py-1.5 rounded-sm text-sm font-medium transition-all duration-150",
              mode === "edit"
                ? "bg-surface-primary text-text-primary shadow-sm"
                : "text-text-tertiary hover:text-text-secondary hover:bg-surface-primary/50"
            )}
            aria-label="Edit mode (Cmd+E to toggle)"
            title="Edit mode (Cmd+E to toggle)"
          >
            <Edit3 size={16} />
            <span>Write</span>
          </button>
          <button
            onClick={() => setMode("preview")}
            className={clsx(
              "flex items-center gap-2 px-3 py-1.5 rounded-sm text-sm font-medium transition-all duration-150",
              mode === "preview"
                ? "bg-surface-primary text-text-primary shadow-sm"
                : "text-text-tertiary hover:text-text-secondary hover:bg-surface-primary/50"
            )}
            aria-label="Preview mode (Cmd+E to toggle)"
            title="Preview mode (Cmd+E to toggle)"
          >
            <Eye size={16} />
            <span>Preview</span>
          </button>
        </div>
      </div>

      {/* Editor Content */}
      <div className="flex flex-1 min-h-0">
        {mode === "edit" ? (
          <textarea
            value={value}
            onChange={handleTextareaChange}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            placeholder={placeholder}
            className={clsx(
              "flex-1 border-none outline-none p-journal font-sans text-journal-body text-text-primary bg-transparent resize-none overflow-y-auto whitespace-pre-wrap break-words transition-all duration-150",
              "placeholder:text-text-muted placeholder:italic",
              focused && "bg-accent-50/30"
            )}
            spellCheck="true"
          />
        ) : (
          <div className="flex-1 p-journal overflow-y-auto">
            {value.trim() ? (
              <div className="prose prose-gray max-w-none">
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={markdownComponents}
                >
                  {value}
                </ReactMarkdown>
              </div>
            ) : (
              <div className="flex items-center justify-center h-full text-text-muted italic">
                <p>Nothing to preview yet. Switch to Write mode to start writing.</p>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}