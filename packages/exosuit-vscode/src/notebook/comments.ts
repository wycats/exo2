import * as vscode from "vscode";
import { exec } from "child_process";
import { promisify } from "util";

const execAsync = promisify(exec);

export class ExosuitCommentController implements vscode.Disposable {
  private controller: vscode.CommentController;
  private disposables: vscode.Disposable[] = [];

  constructor(_context: vscode.ExtensionContext) {
    this.controller = vscode.comments.createCommentController(
      "exo-plan",
      "Exosuit Plan"
    );

    this.controller.commentingRangeProvider = {
      provideCommentingRanges: (
        document: vscode.TextDocument,
        _token: vscode.CancellationToken
      ) => {
        // Only allow comments in .exo.ipynb cells (which are TOML)
        if (document.languageId === "toml") {
          const lineCount = document.lineCount;
          return [new vscode.Range(0, 0, lineCount - 1, 0)];
        }
        return [];
      },
    };

    this.disposables.push(this.controller);

    // Register the command to handle comment submission
    this.disposables.push(
      vscode.commands.registerCommand(
        "exosuit.addComment",
        this.handleAddComment.bind(this)
      )
    );
  }

  private async handleAddComment(reply: vscode.CommentReply) {
    const thread = reply.thread;
    const text = reply.text;
    const uri = thread.uri;

    // 1. Find the Step ID
    // We need to find the notebook cell corresponding to this URI.
    // Iterate over all notebook documents to find the cell.
    let stepId: string | undefined;

    for (const notebook of vscode.workspace.notebookDocuments) {
      for (const cell of notebook.getCells()) {
        if (cell.document.uri.toString() === uri.toString()) {
          // Found the cell. Get ID from metadata.
          // Assuming the serializer puts 'name' or 'id' in metadata.
          // The RFC says "Projects each step into a Cell".
          // Let's assume metadata.name holds the ID/Title.
          stepId = cell.metadata?.name || cell.metadata?.id;
          break;
        }
      }
      if (stepId) {break;}
    }

    if (!stepId) {
      vscode.window.showErrorMessage(
        "Could not determine Step ID for this comment."
      );
      return;
    }

    // 2. Execute CLI Command
    try {
      // We need the workspace root.
      const workspaceFolder = vscode.workspace.getWorkspaceFolder(uri);
      if (!workspaceFolder) {
        throw new Error("No workspace folder found.");
      }

      const cwd = workspaceFolder.uri.fsPath;
      // Escape quotes in message
      const escapedMessage = text.replace(/"/g, '\\"');

      // Run exo impl add-feedback
      // Assuming 'exo' is in the path or we use the local one.
      // For dev, we might need to use 'cargo run'.
      // But in production, it should be 'exo'.
      // Let's try to find the exo binary or use a configured path.
      // For now, hardcode to 'cargo run -p exo --' if in dev, or 'exo' otherwise.
      // Better: use the task definition or a configuration.
      // I'll assume 'exo' is available or I'll use the relative path for this workspace.

      const cmd = `cargo run -p exo -- impl add-feedback --step-id "${stepId}" --message "${escapedMessage}" --author "User"`;

      await execAsync(cmd, { cwd });

      // 3. Optimistic Update (Optional, but good for UX)
      // We create a new comment in the thread to show it immediately.
      const newComment = new ExosuitComment(text, vscode.CommentMode.Preview, {
        name: "User",
      });
      thread.comments = [...thread.comments, newComment];
    } catch (error: any) {
      vscode.window.showErrorMessage(
        `Failed to add feedback: ${error.message}`
      );
    }
  }

  dispose() {
    this.disposables.forEach((d) => d.dispose());
  }
}

class ExosuitComment implements vscode.Comment {
  label: string | undefined;
  constructor(
    public body: string | vscode.MarkdownString,
    public mode: vscode.CommentMode,
    public author: vscode.CommentAuthorInformation,
    public contextValue?: string
  ) {}
}
