# dirscribe

A CLI tool that collects and combines files with specific extensions from a directory into a single output file.

## Features

- Recursive directory traversal
- Multiple file extensions support 
- Gitignore rules support
- Path inclusion/exclusion
- Keyword filtering
- Clear file separation in output

## Installation

```bash
cargo install dirscribe
```

## Usage

Basic syntax:
```bash
dirscribe <directory_path> <comma_separated_suffixes> [options]
```

Example:
```bash
dirscribe ./src txt,md,rs
```

### Options

- `--use-gitignore`: Respect .gitignore rules
- `--exclude-paths`: Comma-separated paths to exclude
- `--include-paths`: Comma-separated paths to include
- `--or-keywords`: Only include files containing at least one of these keywords
- `--and-keywords`: Only include files containing all of these keywords
- `--exclude-keywords`: Exclude files containing any of these keywords
- `--diff-only`: Only process files that have Git changes
- `--start-commit-id`: Starting commit ID for Git diff range (optional). If provided alone without end-commit-id, diffs from this commit to the current working directory
- `--end-commit-id`: Ending commit ID for Git diff range (optional). Must be used with start-commit-id

### Advanced Example

```bash
# Example using Git commit range
dirscribe ./src rs,md \
  --diff-only \
  --start-commit-id abc123 \
  --end-commit-id def456
```

This will only process files that changed between commits abc123 and def456.

### Advanced Example with All Options

```bash
dirscribe ./src rs,md \
  --use-gitignore \
  --include-paths src \
  --exclude-paths src/core,src/temp \
  --or-keywords "TODO,FIXME" \
  --and-keywords "pub,struct" \
  --exclude-keywords "DEPRECATED,WIP" \
  --diff-only
```

## Output Format

The tool creates `dirscribe.txt` in the current directory with entries in this format:

```
File: /path/to/file1.txt
[Contents of file1.txt]

File: /path/to/file2.md
[Contents of file2.md]
```

## License

MIT License
