import { CommandCategory, type Command, type SearchableCommand } from "../types/commands";

export class CommandRegistry {
  private commands = new Map<string, Command>();

  constructor() {
    this.registerBuiltIns();
  }

  register(command: Command) {
    this.commands.set(command.id, command);
  }

  unregister(id: string) {
    this.commands.delete(id);
  }

  getAll(): Command[] {
    return Array.from(this.commands.values());
  }

  search(query: string): SearchableCommand[] {
    const q = query.trim().toLowerCase();
    const all = this.getAll();
    if (!q) {
      return all.map((command) => ({ command, score: 1 }));
    }

    return all
      .map((command) => {
        const haystack = [
          command.id,
          command.name,
          command.description,
          ...(command.keywords || []),
        ]
          .join(" ")
          .toLowerCase();

        const score = this.fuzzyScore(q, haystack);
        return { command, score } as SearchableCommand;
      })
      .filter((c) => c.score > 0)
      .sort((a, b) => b.score - a.score);
  }

  // Very small, cheap fuzzy scoring: substring match + token overlap
  private fuzzyScore(query: string, text: string): number {
    if (!text) return 0;
    if (text.includes(query)) return query.length / text.length + 0.5;

    const qTokens = query.split(/\s+/g).filter(Boolean);
    const tTokens = new Set(text.split(/\s+/g).filter(Boolean));
    if (!qTokens.length) return 0;

    let matches = 0;
    for (const t of qTokens) {
      if (tTokens.has(t)) matches++;
    }
    return matches / qTokens.length;
  }

  private registerBuiltIns() {
    const stub = (message: string) => async () => {
      console.log(message);
      return { success: true, message } as const;
    };

    this.register({
      id: "thread.new",
      name: "New Thread",
      description: "Create a new conversation thread",
      category: CommandCategory.THREAD,
      keywords: ["create", "start", "new", "thread", "chat"],
      icon: "💬",
      execute: stub("[STUB] New Thread command executed"),
    });

    this.register({
      id: "thread.search",
      name: "Search Threads",
      description: "Find threads by content or title",
      category: CommandCategory.SEARCH,
      keywords: ["search", "find", "filter", "threads"],
      icon: "🔍",
      execute: stub("[STUB] Search Threads command executed"),
    });

    this.register({
      id: "nav.settings",
      name: "Open Settings",
      description: "Open application settings",
      category: CommandCategory.NAVIGATION,
      keywords: ["settings", "+preferences", "config", "options"],
      icon: "⚙️",
      execute: stub("[STUB] Open Settings command executed"),
    });

    this.register({
      id: "export.thread",
      name: "Export Thread",
      description: "Export current thread to file",
      category: CommandCategory.EXPORT,
      keywords: ["export", "save", "download", "backup"],
      icon: "💾",
      execute: stub("[STUB] Export Thread command executed"),
    });

    this.register({
      id: "system.shell",
      name: "Run Shell Command",
      description: "Execute a shell command",
      category: CommandCategory.SYSTEM,
      keywords: ["shell", "terminal", "command", "cli", "bash"],
      icon: "⚡",
      execute: async (args) => {
        console.log("[STUB] Run Shell Command with args:", args);
        return { success: true, message: "Shell command executed (stub)" };
      },
    });
  }
}

export const commandRegistry = new CommandRegistry();
