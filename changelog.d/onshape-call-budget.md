### Command line and install

- The Onshape fetch no longer stops ordinary documents at 50 calls. Without `--max-calls` (or `max_calls` in the MCP tool), the limit grows with the document's Part Studios, assemblies and parts, up to a ceiling of 2000 calls; `--max-calls N` still sets a fixed limit.
- In the app, a fetch that stops at the request limit now says so in plain words and the Results card offers Continue fetching, which carries on from the replies already fetched. Other fetch errors, and a crash during the fetch, end the job with a message too.
