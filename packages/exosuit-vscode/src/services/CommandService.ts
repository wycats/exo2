import * as vscode from "vscode";

export class CommandService {
  #commands = new Set<string>();
  #disposables: vscode.Disposable[] = [];

  /**
   * Registers a Global Singleton command (e.g., defined in package.json).
   * Throws if the command is already registered by this service.
   */
  registerGlobal(
    id: string,
    callback: (...args: any[]) => any
  ): vscode.Disposable {
    if (this.#commands.has(id)) {
      throw new Error(`Command "${id}" is already registered.`);
    }

    const disposable = vscode.commands.registerCommand(id, callback);
    this.#commands.add(id);
    this.#disposables.push(disposable);

    return {
      dispose: () => {
        disposable.dispose();
        this.#commands.delete(id);
        const idx = this.#disposables.indexOf(disposable);
        if (idx !== -1) {
          this.#disposables.splice(idx, 1);
        }
      },
    };
  }

  /**
   * Registers a uniquely scoped command for a component instance.
   * Returns the generated Command ID and a dispose function.
   */
  registerScoped(
    baseName: string,
    callback: (...args: any[]) => any
  ): { id: string; dispose: () => void } {
    const uniqueId = crypto.randomUUID().split("-")[0]; // Short UUID for readability
    const id = `${baseName}.${uniqueId}`;

    const disposable = vscode.commands.registerCommand(id, callback);
    this.#commands.add(id);
    this.#disposables.push(disposable);

    return {
      id,
      dispose: () => {
        disposable.dispose();
        this.#commands.delete(id);
        const idx = this.#disposables.indexOf(disposable);
        if (idx !== -1) {
          this.#disposables.splice(idx, 1);
        }
      },
    };
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#disposables = [];
    this.#commands.clear();
  }
}

export const commandService = new CommandService();

// Proxy for ergonomic command execution: commands.exosuit.increment()
export const commands = new Proxy(
  {},
  {
    get(_target, section: string) {
      return new Proxy(
        {},
        {
          get(_target, key: string) {
            const commandId = `${section}.${key}`;
            return (...args: any[]) => {
              return vscode.commands.executeCommand(commandId, ...args);
            };
          },
        }
      );
    },
  }
) as any;
