### Desktop app

- SolidCraft plugin: opening a Fusion design with design history no longer stops with "wasm trap: unreachable instruction". The Fusion reader started threads, which the plugin's WebAssembly build does not have; it now runs those steps one after another there and gives the same result. If the plugin ever stops on an internal error again, SolidCraft's log names the cause.
