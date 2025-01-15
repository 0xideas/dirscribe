# dirscribe

A CLI tool that collects and combines files with specific extensions from a directory into a single output. The output is copied to the clipboard by default.

## Features and Options

- Recursive traverse directory and filter by file extension
- Apply .gitignore 
- Configure subpaths to include or exclude
- Filter by positive and/or negative keyword filters
- Only output diff, between commit ids or from a specified commit id to the current state
- Embed output in prompt template
- Write output to file

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
dirscribe . md,rs
```

### Options

- `--exclude-paths`: Comma-separated paths to exclude
- `--include-paths`: Comma-separated paths to include
- `--or-keywords`: Only include files containing at least one of these keywords
- `--and-keywords`: Only include files containing all of these keywords
- `--exclude-keywords`: Exclude files containing any of these keywords
- `--diff-only`: Only process files that have Git changes
- `--start-commit-id`: Starting commit ID for Git diff range (optional). If provided alone without end-commit-id, diffs from this commit to the current working directory
- `--end-commit-id`: Ending commit ID for Git diff range (optional). Must be used with start-commit-id
- `--prompt-template-path`: Path to a template file that will wrap the output. The template must contain the placeholder `${${CONTENT}$}$` where the collected content should be inserted
- `--output-path`: Path where the output file should be written. If not provided, output will be copied to clipboard
- `--dont-use-gitignore`: include files covered by .gitignore

### Advanced Example

```bash
# Example using Git commit range
dirscribe . rs,md \
  --diff-only \
  --start-commit-id abc123 \
  --end-commit-id def456
```

This will only process files that changed between commits abc123 and def456.

### Advanced Example with All Options

```bash
dirscribe . rs,md \
  --exclude-paths src/core,src/temp \
  --or-keywords "TODO,FIXME" \
  --prompt-template-path "summarize-issues-to-address-prompt.txt"
```

## Output Format

The output is in this format:

```
File Paths:
/path/to/file1.txt
/path/to/file2.md

File Contents:
File: /path/to/file1.txt
[Contents of file1.txt]

File: /path/to/file2.md
[Contents of file2.md]
```

If a prompt template path is specified, this output will be embedded in that template for the final output.

## Template

You can specify a template to embed the output in. The template should be a txt file that contains the string "${${CONTENT}$}$" (without quotation marks), and that string will be replaced with the output as shown above.

## License

MIT License
