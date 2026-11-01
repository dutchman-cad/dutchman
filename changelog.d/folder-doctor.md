### Command line and install

- **`dutchman doctor <folder>`** checks a whole folder or zip on your machine, with CPU and memory
  budgets, private checkpoints so it can resume, and an anonymous report. Repeat failures are
  grouped by signature. Nothing is sent.
- **`dutchman diagnose --step`** compares a design with its matching STEP export, worst part first.
  A STEP beside the file is found by itself. The comparison limits are advisory for now.

### Desktop app

- **Check a folder** and **Add matching STEP** bring the same checks to the app, and the MCP server
  adds `doctor`, `diagnose`, and `compare_step`.
