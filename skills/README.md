# Agent skills

Skills for AI assistants that convert CAD files with Dutchman, through its
[MCP server](../docs/mcp.md) or the `dutchman` command. Each folder is one skill in the
[Agent Skills](https://agentskills.io) format: a `SKILL.md` whose name and description tell
the assistant when to load it.

| Skill | Loads when the assistant |
|---|---|
| [`dutchman-converting`](dutchman-converting/SKILL.md) | is asked to turn a Fusion, Inventor or SolidWorks file into a SolidCraft design |

It teaches the assistant to:

- check the format, and say plainly what to do with formats Dutchman does not convert
- inspect a file before converting it
- check the result in SolidCraft instead of trusting the reply alone
- explain the loss report in plain words
- keep the user's designs on the user's machine

Set up the MCP server first ([docs/mcp.md](../docs/mcp.md)). To check results in SolidCraft,
the skill also uses `solidcraft-cli` or SolidCraft's own MCP server; SolidCraft's skills
(in the SolidCraft repository's `skills/` folder) cover editing the converted design.

## Install

Claude Code, for all projects:

```sh
git clone --depth 1 https://github.com/dutchman-cad/dutchman
mkdir -p ~/.claude/skills
cp -r dutchman/skills/dutchman-converting ~/.claude/skills/
```

Windows (PowerShell):

```powershell
git clone --depth 1 https://github.com/dutchman-cad/dutchman
New-Item -ItemType Directory -Force $HOME\.claude\skills | Out-Null
Copy-Item -Recurse dutchman\skills\dutchman-converting $HOME\.claude\skills\
```

- **One project only:** copy the folder into that project's `.claude/skills/` instead.
- **Other assistants:** for those that read the Agent Skills format, copy the folder into the
  skills directory their documentation names.
- **Updating:** copy the folder again after you update Dutchman.

## Maintaining

When a tool, an argument, a report field or a message these files describe changes, update
them in the same pull request.
