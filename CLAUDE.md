# Project Workspace — Costa OS Baseline

This project runs on a Costa OS workstation. The following tools and workflows are available and should be used.

## System Agents

For server operations, deployments, SSH, and builds, use the Costa OS agent system instead of running raw commands (doctl, ssh, scp, rsync, etc.):

```bash
costa-agents run deployer "deploy this app to production"
costa-agents run sysadmin "check disk usage on the droplet"
costa-agents run builder "build the ISO"
```

Agent definitions: `~/projects/costa-os/configs/costa/agents/*.yaml`

## Obsidian Vault

Use the Obsidian MCP server to read/write notes to `~/notes/`:
- **Progress & decisions** → `~/notes/daily/` (append to today's date)
- **User corrections** → `~/notes/feedback/`
- **Project context** → `~/notes/projects/`

## Available Skills — USE THESE FIRST

**Before implementing anything manually, check if a skill or plugin already handles it.** Skills produce better results than ad-hoc implementation. The system has plugins from the Claude Code marketplace — check what's available in the system-reminder at session start.

Installed plugins (user-level, available in all projects):
- `/frontend-design` — production-grade UI/UX with animations, typography, motion. **Use this for any visual/frontend work** instead of writing CSS/animations by hand
- `/feature-dev` — 7-phase structured development workflow
- `/code-review` — parallel agent code review
- `/commit` — analyze changes and commit
- `/deploy` — full deploy pipeline (test, commit, push, agent, healthcheck)
- `/note` — write to Obsidian vault with frontmatter

**Rule:** If a skill covers what you're about to do, invoke it. Don't reinvent what a skill already does well.

## MCP Servers

- **costa-system** — system commands, screen reading, window interaction, navigation

## Additional MCP Servers

- **context7** — fetch version-specific library docs on demand. Use when unsure about API signatures.
- **claude-code-enhanced** — delegate mechanical subtasks (tests, lint, metrics) to a child Claude session. Each delegation = full API call, so use for token-heavy mechanical work only.
- **code-review-graph** — AST knowledge graph for reviews. Query call chains, type hierarchies, symbol refs instead of reading entire files. Available in projects with `.mcp.json`.

## Rules

- Use CLI wrappers (`cli-anything-*`) before screen reading or screenshots
- Don't SSH to servers manually — use deployer/sysadmin agents
- Don't use raw `doctl`/cloud CLI for infrastructure — use agents
- Save findings and progress to Obsidian, not just terminal output
- See `~/CLAUDE.md` for full system rules and preferences
