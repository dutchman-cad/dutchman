### Command line and install

- `convert-all` now schedules a batch middle-out: the largest files start first (assemblies and multi-document archives ahead of the rest), and each file starts only when its predicted memory fits the memory free now, so small files fill the gaps around big ones and big ones no longer run together out of memory. Reports, the journal and printed lines stay in input order; `--jobs` still caps how many run at once, and `--jobs 1` is unchanged.
