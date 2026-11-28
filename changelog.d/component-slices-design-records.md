### Assemblies

- A large Fusion archive no longer copies the whole design layer into every component it cuts out: a component keeps the design records of its own features, so a 497-component archive needs a fraction of the memory it did. Components are also cut out of the decoded model directly, not written out as JSON and read back. Converted results are unchanged.
