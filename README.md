# statusline

A [Claude Code status line](https://code.claude.com/docs/en/statusline), styled after [Pure](https://github.com/sindresorhus/pure):

```
Opus 4.8 (1M context) ~/statusline master 44k/1M (4%)
```

## Installation

Build `statusline`:

```bash
cargo build --release && cp target/release/statusline ~/.claude/statusline
```

… and then point `~/.claude/settings.json` at it:

```json
{
  "statusLine": {
    "type": "command",
    "command": "~/.claude/statusline"
  }
}
```

## License

[MIT](./LICENSE)
