import * as vscode from "vscode";

class ConfigSignal<T> {
  private _version = 0;
  private _onDidChange = new vscode.EventEmitter<T | undefined>();
  public readonly onDidChange = this._onDidChange.event;

  #section: string;
  #key: string;
  #value: T | undefined;

  constructor(section: string, key: string) {
    this.#section = section;
    this.#key = key;
    this.#update();
  }

  #update() {
    const config = vscode.workspace.getConfiguration(this.#section);
    const newValue = config.get<T>(this.#key);
    if (newValue !== this.#value) {
        this.#value = newValue;
        this._version += 1;
        this._onDidChange.fire(this.#value);
    }
  }

  check(e: vscode.ConfigurationChangeEvent) {
    if (e.affectsConfiguration(`${this.#section}.${this.#key}`)) {
      this.#update();
    }
  }

  get value() {
    return this.#value;
  }
  
  get version() {
      return this._version;
  }
}

export class ConfigurationService {
  #signals = new Map<string, WeakRef<ConfigSignal<any>>>();
  #disposables: vscode.Disposable[] = [];

  init() {
    this.#disposables.push(
      vscode.workspace.onDidChangeConfiguration((e) => {
        for (const [key, ref] of this.#signals.entries()) {
          const signal = ref.deref();
          if (!signal) {
            this.#signals.delete(key);
            continue;
          }
          signal.check(e);
        }
      })
    );
  }

  getSignal<T>(section: string, key: string): ConfigSignal<T> {
    const id = `${section}.${key}`;
    const ref = this.#signals.get(id);
    let signal = ref?.deref();

    if (!signal) {
      signal = new ConfigSignal<T>(section, key);
      this.#signals.set(id, new WeakRef(signal));
    }
    return signal;
  }

  dispose() {
    this.#disposables.forEach((d) => d.dispose());
    this.#signals.clear();
  }
}

const service = new ConfigurationService();

export const configurationService = service;

export const config = new Proxy(
  {},
  {
    get(_target, section: string) {
      return new Proxy(
        {},
        {
          get(_target, key: string) {
            return service.getSignal(section, key).value;
          },
        }
      );
    },
  }
) as any;
