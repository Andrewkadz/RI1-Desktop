export enum CommandCategory {
  THREAD = "thread",
  DOCUMENT = "document",
  WORKSPACE = "workspace",
  SYSTEM = "system",
  NAVIGATION = "navigation",
  EXPORT = "export",
  SEARCH = "search",
}

export interface CommandArgument {
  name: string;
  description: string;
  required?: boolean;
}

export interface CommandResult {
  success: boolean;
  message?: string;
  data?: any;
}

export interface AppContext {
  activeThreadId: string | null;
  // Future phases may extend with: activeThread, workspace, selection, etc.
}

export interface Command {
  id: string;
  name: string;
  description: string;
  category: CommandCategory;
  keywords: string[];
  icon?: string;
  arguments?: CommandArgument[];
  execute: (args: Record<string, any>, context: AppContext) => Promise<CommandResult>;
}

export interface SearchableCommand {
  command: Command;
  score: number;
}
