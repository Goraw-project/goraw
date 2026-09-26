# Goraw Language Support for Visual Studio Code

Extension for the **Goraw** systems programming language (`.gw`).

## Features

- **Full Syntax Highlighting**:
  - Goraw keywords, declarations, control flow, and modifiers
  - Built-in types (`i8`..`i64`, `u8`..`u64`, `f32`, `f64`, `bool`, `void`, `str`)
  - Standard library collections (`Vec`, `HashMap`, `Result`, `Option`, `Box`, `File`)
  - The `?` (Try) error operator
  - Numeric constants (decimal, hex `0x`, binary `0b`, octal `0o`, float)
  - String literals and escape sequences
- **Embedded Language Blocks**:
  - `c { ... }` — embedded C syntax highlighting
  - `cpp { ... }` — embedded C++ syntax highlighting
  - `asm { ... }` — embedded Assembly syntax highlighting
- **Language Configuration**:
  - Bracket matching and auto-closing
  - Line (`//`) and block (`/* ... */`) comments
  - Auto-indentation

## Installation

To use this extension locally:

1. Copy or symlink `editors/vscode` to your VS Code extensions directory:
   - Windows: `%USERPROFILE%\.vscode\extensions\goraw-vscode`
   - Linux / macOS: `~/.vscode/extensions/goraw-vscode`
2. Restart or reload VS Code (`Developer: Reload Window`).
3. Open any `.gw` file to enjoy full syntax highlighting!
