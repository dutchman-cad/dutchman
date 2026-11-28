### Formats

- An Onshape fetch no longer stops one call short when the document needs just over 50 calls: the limit now fits the estimate you were shown, in the app, on the command line with `--yes`, and in the MCP tool.
- When a fetch does stop at its call limit, the app says how many calls were spent, how many the document needs in all, and to choose Continue fetching; the command line says the same and names the `--max-calls` value that finishes it.
