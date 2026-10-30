# Pilot

`dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot), a local helper for
driving SolidCraft from plain-language requests through its public interfaces.

```sh
dutchman pilot --help
dutchman pilot how "round all edges 3 mm"
dutchman pilot model --upstream http://localhost:11434/v1
```

Install SolidCraft separately. The `model` command connects to a chat-completions endpoint
such as Ollama or LM Studio. Connect SolidCraft to the model proxy at
`http://127.0.0.1:11500/v1`. Pilot is included with the default `pilot` Cargo feature.

`dutchman` also runs the other Dutchman CAD tools: `dutchman pilot` and `dutchman fixtures`
run installed `dutchman-pilot` and `dutchman-fixtures` executables, found next to `dutchman`
first, then on `PATH`. `dutchman --help` lists the installed tools and their versions. An
installed Pilot takes precedence over the built-in one.
