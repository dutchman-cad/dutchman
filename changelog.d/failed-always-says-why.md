### Reports

- A file that fails in a batch always says what stopped it, with its stage, kind, reason code and a one-line message in the report and `failed.json`: a converter killed by a signal (signal 9 says it was likely out of memory), a panic with its message, an exit with no report, an archive design that could not be read, and an error that came back with no text. A panic in one file no longer ends the whole batch.
- An assembly with parts that could not be placed now carries a `reason` saying so, instead of a bare failed result.
