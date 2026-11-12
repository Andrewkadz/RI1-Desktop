import React, { useEffect, useMemo, useRef, useState } from "react";
import { commandRegistry } from "../../services/commandRegistry";
import type { AppContext, Command } from "../../types/commands";
import "./styles.css";

interface Props {
  open: boolean;
  onClose: () => void;
  appContext: AppContext;
}

export const CommandPalette: React.FC<Props> = ({ open, onClose, appContext }) => {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelectedIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const results = useMemo(() => {
    return commandRegistry.search(query);
  }, [query]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, Math.max(results.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = results[selectedIndex];
      if (item) {
        void runCommand(item.command);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  const runCommand = async (command: Command) => {
    try {
      await command.execute({}, appContext);
    } catch (e) {
      console.error("Command execution error", e);
    } finally {
      onClose();
    }
  };

  if (!open) return null;

  return (
    <div className="command-palette-backdrop" onClick={onClose}>
      <div className="command-palette" onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="command-palette-input"
          placeholder="Type a command or search..."
          value={query}
          onChange={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={handleKeyDown}
        />
        <div className="command-palette-list">
          {results.map(({ command }, idx) => (
            <div
              key={command.id}
              className={
                "command-palette-item" + (idx === selectedIndex ? " selected" : "")
              }
              onMouseDown={(e) => {
                e.preventDefault();
                void runCommand(command);
              }}
            >
              <div className="command-palette-item-title">
                {command.icon && <span style={{ marginRight: 6 }}>{command.icon}</span>}
                {command.name}
              </div>
              <div className="command-palette-item-description">{command.description}</div>
            </div>
          ))}
          {results.length === 0 && (
            <div style={{ padding: 8, fontSize: 12, opacity: 0.7 }}>
              No commands found. Try a different search.
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
