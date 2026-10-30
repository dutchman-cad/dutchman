# Pilot

`dutchman pilot` runs [Pilot](https://github.com/dutchman-cad/pilot), a local helper for
driving SolidCraft from plain-language requests through its public interfaces. Find
commands and example calls:

```sh
dutchman pilot how "round all edges 3 mm"
```

Start its model proxy with a chat-completions endpoint such as Ollama or LM Studio:

```sh
dutchman pilot model --upstream http://localhost:11434/v1
```

Connect SolidCraft to `http://127.0.0.1:11500/v1`. Install SolidCraft separately.
`dutchman pilot --help` lists Pilot's commands; `dutchman pilot model --help` lists the
model proxy's options. The default `pilot` Cargo feature includes this command.
