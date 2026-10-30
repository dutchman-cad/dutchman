### Reports

- Save anonymous diagnostic JSONL logs during conversion, including stages, resource heartbeats,
  Fusion member sizes and decode attempts, and final loss counts. CLI, MCP, and desktop settings
  support fixed diagnostic folders with ten-log retention; the app reopens unfinished logs after
  a restart. Each record is flushed immediately and synced at least every five seconds.
