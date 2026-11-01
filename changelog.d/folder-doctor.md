### Command line and install

- **`dutchman doctor <folder>`** checks a whole folder or zip on your machine, with CPU and memory
  budgets, private checkpoints so it can resume, and an anonymous report. Repeat failures are
  grouped by signature. Nothing is sent.
- **`dutchman diagnose --step`** compares a design with its matching STEP export, worst part first.
  A STEP beside the file is found by itself. The comparison limits are advisory for now.
- The desktop app can check a folder and take a matching STEP, and the MCP server adds `doctor`,
  `diagnose`, and `compare_step`.
