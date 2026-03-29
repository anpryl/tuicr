# Design: `--stdin` flag for tuicr

## Goal

Add a `--stdin` flag that reads content from stdin and presents it for annotation, similar to `--file` but without requiring a file on disk. Primary use case: Claude Code `PermissionRequest` hook on `ExitPlanMode` pipes plan content via JSON stdin.

## Usage

```bash
# Read raw content from stdin
echo "# My Plan\n\n1. Do X\n2. Do Y" | tuicr --stdin

# With a display name (shown in file list / status bar)
echo "..." | tuicr --stdin --stdin-name "plan.md"

# Real-world: Claude Code hook
jq -r '.tool_input.plan' | tuicr --stdin --stdin-name "plan.md" --stdout
```

## Implementation

### Files to modify

#### 1. `src/theme/mod.rs` — CLI parsing

Add two new fields to `CliArgs`:

```rust
pub struct CliArgs {
    // ... existing fields ...
    /// Read content from stdin for annotation (no VCS/file required)
    pub stdin_mode: bool,
    /// Display name for stdin content (default: "stdin")
    pub stdin_name: Option<String>,
}
```

Add parsing in `parse_cli_args_from()`:
- `--stdin` → sets `stdin_mode = true`
- `--stdin-name <name>` / `--stdin-name=<name>` → sets `stdin_name`

Add mutual exclusivity checks (same as `--file`):
- `--stdin` cannot combine with `--file`, `--path`, `-r`, `-w`

#### 2. `src/vcs/stdin.rs` — New `StdinBackend` (copy of `FileBackend`)

```rust
pub struct StdinBackend {
    info: VcsInfo,
    content: String,
    display_name: String,
}

impl StdinBackend {
    pub fn new(content: String, display_name: Option<String>) -> Result<Self> {
        let name = display_name.unwrap_or_else(|| "stdin".to_string());
        let info = VcsInfo {
            root_path: PathBuf::from("."),
            head_commit: "stdin".to_string(),
            branch_name: None,
            vcs_type: VcsType::Stdin,
        };
        Ok(Self { info, content, display_name: name })
    }
}
```

Implement `VcsBackend`:
- `get_working_tree_diff()` — same logic as `FileBackend` but reads from `self.content` instead of filesystem. Uses `display_name` for syntax detection (extension-based highlighting).
- `fetch_context_lines()` — reads from `self.content` (it's all in memory).

#### 3. `src/vcs/traits.rs` — Add `VcsType::Stdin`

```rust
pub enum VcsType {
    Git,
    Mercurial,
    Jujutsu,
    File,
    Stdin,  // new
}
```

Add `Display` impl: `VcsType::Stdin => write!(f, "stdin")`.

#### 4. `src/vcs/mod.rs` — Export `StdinBackend`

```rust
mod stdin;
pub use stdin::StdinBackend;
```

#### 5. `src/main.rs` — Wire up stdin reading

Before `App::new()`, add stdin reading logic:

```rust
// --stdin mode: read all of stdin before entering raw mode
let stdin_content = if cli_args.stdin_mode {
    use std::io::Read;
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)
        .expect("Failed to read from stdin");
    if buf.is_empty() {
        eprintln!("Error: stdin is empty");
        std::process::exit(2);
    }
    Some(buf)
} else {
    None
};
```

**Critical**: stdin must be read **before** `enable_raw_mode()` — raw mode changes terminal input behavior.

Pass to `App::new()` as a new parameter:
```rust
cli_args.stdin_content,  // Option<String>
cli_args.stdin_name,     // Option<String>
```

#### 6. `src/app.rs` — Handle stdin in `App::new()`

Add new parameters and a new branch before the `--file` check:

```rust
pub fn new(
    // ... existing params ...
    file_path: Option<&str>,
    stdin_content: Option<String>,
    stdin_name: Option<String>,
) -> Result<Self> {
    // --stdin mode
    if let Some(content) = stdin_content {
        let vcs = Box::new(StdinBackend::new(content, stdin_name)?);
        // ... same pattern as --file mode ...
    }
    // --file mode (existing)
    if let Some(file_path) = file_path {
        // ...
    }
```

### Syntax highlighting

`StdinBackend` determines syntax from the `display_name` extension:
- `--stdin-name plan.md` → markdown highlighting
- `--stdin-name config.nix` → nix highlighting
- No name / no extension → plain text (no highlighting)

This reuses `SyntaxHighlighter::highlight_file_lines()` by passing a synthetic `PathBuf` with the display name.

### Persistence (session caching)

Stdin content has no stable file path, so session caching uses a content hash:
- Session key: `sha256(content)[..16]` + display_name
- Stored in same location as other sessions: `~/Library/Application Support/tuicr/reviews/`
- This means re-reviewing the same plan reloads previous annotations

### Testing

Add to existing CLI arg tests in `src/theme/mod.rs`:

```rust
#[test]
fn stdin_flag() {
    let args = parse_for_test(&["tuicr", "--stdin"]);
    assert!(args.unwrap().stdin_mode);
}

#[test]
fn stdin_with_name() {
    let args = parse_for_test(&["tuicr", "--stdin", "--stdin-name", "plan.md"]);
    let args = args.unwrap();
    assert!(args.stdin_mode);
    assert_eq!(args.stdin_name, Some("plan.md".to_string()));
}

#[test]
fn stdin_conflicts_with_file() {
    let args = parse_for_test(&["tuicr", "--stdin", "--file", "foo.md"]);
    assert!(args.is_err());
}
```

## Alternatives considered

1. **Temp file in wrapper** — works today but loses the "pipe" ergonomics and adds cleanup burden
2. **`--file -` convention** — confusing because `-` isn't a real path; `FileBackend::new` would need special-casing
3. **Named pipe/FIFO** — fragile, platform-dependent, no advantage over temp file

## Scope

- ~150 lines of new Rust code (mostly `StdinBackend`, mirroring `FileBackend`)
- ~30 lines of CLI parsing additions
- ~10 lines of wiring in `main.rs` and `app.rs`
- No new dependencies
