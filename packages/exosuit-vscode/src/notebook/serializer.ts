import * as vscode from "vscode";
import { TextDecoder, TextEncoder } from "util";
// import { parse } from "smol-toml";

export class ExosuitNotebookSerializer implements vscode.NotebookSerializer {
  async deserializeNotebook(
    content: Uint8Array,
    _token: vscode.CancellationToken
  ): Promise<vscode.NotebookData> {
    const contents = new TextDecoder().decode(content);

    // Log serializer usage
    if (
      vscode.workspace.workspaceFolders &&
      vscode.workspace.workspaceFolders.length > 0
    ) {
      const logPath = vscode.Uri.joinPath(
        vscode.workspace.workspaceFolders[0].uri,
        "serializer.txt"
      );
      vscode.workspace.fs.writeFile(
        logPath,
        new Uint8Array(
          Buffer.from(
            `Deserialize called. Content length: ${contents.length}\n`
          )
        )
      );
    }

    const cells: vscode.NotebookCellData[] = [];

    // 1. Try to parse TOML to get metadata for the header
    try {
      // const toml = parse(contents);
      const toml = {} as any;
      const phase = (toml as any).phase;
      if (phase) {
        const title = phase.title || "Untitled Phase";
        const id = phase.id || "no-id";
        const markdown = `# ${title}\n**ID**: \`${id}\``;
        cells.push(
          new vscode.NotebookCellData(
            vscode.NotebookCellKind.Markup,
            markdown,
            "markdown"
          )
        );
      } else {
        cells.push(
          new vscode.NotebookCellData(
            vscode.NotebookCellKind.Markup,
            "No [phase] section found",
            "markdown"
          )
        );
      }
    } catch (e) {
      // If parsing fails, just show error in a markdown cell
      cells.push(
        new vscode.NotebookCellData(
          vscode.NotebookCellKind.Markup,
          "**Error parsing TOML header**",
          "markdown"
        )
      );
    }

    // 2. Add the full content as a code cell
    cells.push(
      new vscode.NotebookCellData(
        vscode.NotebookCellKind.Code,
        contents,
        "toml"
      )
    );

    return new vscode.NotebookData(cells);
  }

  async serializeNotebook(
    data: vscode.NotebookData,
    _token: vscode.CancellationToken
  ): Promise<Uint8Array> {
    // Find the code cell containing the TOML
    // We assume it's the one with language 'toml'

    for (const cell of data.cells) {
      if (
        cell.kind === vscode.NotebookCellKind.Code &&
        cell.languageId === "toml"
      ) {
        return new TextEncoder().encode(cell.value);
      }
    }

    return new TextEncoder().encode("");
  }
}
