import * as vscode from "vscode";
import { executeDirective } from "./directives";
import { getLogger } from "../logging";

const logger = getLogger("extension");

export class ExosuitNotebookController {
  readonly controllerId = "exosuit-notebook-controller";
  readonly notebookType = "exosuit-plan";
  readonly label = "Exosuit Kernel";
  readonly supportedLanguages = ["toml", "markdown"];

  private readonly _controller: vscode.NotebookController;
  private _executionOrder = 0;

  constructor() {
    this._controller = vscode.notebooks.createNotebookController(
      this.controllerId,
      this.notebookType,
      this.label,
    );

    this._controller.supportedLanguages = this.supportedLanguages;
    this._controller.supportsExecutionOrder = true;
    this._controller.executeHandler = this._execute.bind(this);
  }

  dispose() {
    this._controller.dispose();
  }

  private async _execute(
    cells: vscode.NotebookCell[],
    _notebook: vscode.NotebookDocument,
    _controller: vscode.NotebookController,
  ): Promise<void> {
    for (const cell of cells) {
      await this._doExecution(cell);
    }
  }

  private async _doExecution(cell: vscode.NotebookCell): Promise<void> {
    const execution = this._controller.createNotebookCellExecution(cell);
    execution.executionOrder = ++this._executionOrder;
    execution.start(Date.now()); // Keep running time

    try {
      // Clear previous outputs
      await execution.clearOutput();

      const text = cell.document.getText();
      const outputItems: vscode.NotebookCellOutputItem[] = [];

      if (cell.kind === vscode.NotebookCellKind.Markup) {
        // Markdown cells are not executed by the kernel usually, but if they are passed here
        // we can just mark them as success.
        execution.end(true, Date.now());
        return;
      }

      if (text.trim().startsWith("@")) {
        // Directive
        const result = await executeDirective(text);
        outputItems.push(
          vscode.NotebookCellOutputItem.text(result, "text/plain"),
        );
      } else if (text.trim().startsWith("exo ")) {
        // Implicit @run for exo commands
        const result = await executeDirective(`@run: ${text}`);

        // Try to parse as JSON for RTD
        try {
          const json = JSON.parse(result);
          outputItems.push(vscode.NotebookCellOutputItem.json(json));
        } catch {
          // Not JSON
        }
        outputItems.push(
          vscode.NotebookCellOutputItem.text(result, "text/plain"),
        );
      } else {
        // Plain text echo
        outputItems.push(
          vscode.NotebookCellOutputItem.text(text, "text/plain"),
        );
      }

      await execution.replaceOutput([
        new vscode.NotebookCellOutput(outputItems),
      ]);
      execution.end(true, Date.now());
    } catch (err) {
      logger.error("Exosuit Notebook Execution Failed:", err);
      execution.replaceOutput([
        new vscode.NotebookCellOutput([
          vscode.NotebookCellOutputItem.error(err as Error),
        ]),
      ]);
      execution.end(false, Date.now());
    }
  }
}
