# Adding Goraw to GitHub Linguist

This document describes the configuration and steps to register the **Goraw** programming language in [github-linguist/linguist](https://github.com/github-linguist/linguist) so that GitHub natively recognizes `.gw` files, calculates repository statistics, and assigns the official Goraw flame color.

---

## 1. Definition for `lib/linguist/languages.yml`

In the upstream `github-linguist/linguist` repository:

```yaml
Goraw:
  type: programming
  color: "#FF6B00"
  aliases:
    - gw
  extensions:
    - ".gw"
  tm_scope: source.goraw
  ace_mode: text
  codemirror_mode: clike
  codemirror_mime_type: text/x-csrc
  language_id: 948201
```

---

## 2. TextMate Grammar Source

Linguist requires a standalone grammar or an open-source extension grammar:
- **Repository**: [https://github.com/Goraw-project/goraw](https://github.com/Goraw-project/goraw)
- **Path**: `editors/vscode/syntaxes/goraw.tmLanguage.json`
- **Scope**: `source.goraw`
- **License**: MIT

---

## 3. Pull Request Requirements Checklist

Before submitting a PR to `github-linguist/linguist`:
- [x] Grammar is open-source (MIT License).
- [x] Syntax highlighting verified in VS Code / Antigravity IDE.
- [x] Real-world code corpus exists on GitHub:
  - Standard library: `std/alloc/*.gw`, `std/crypto/*.gw`, `std/libc/*.gw`, `std/*.gw`
  - Backend modules: `backend/selectiondag/*.gw`, `backend/x86/*.gw`, `backend/linker/*.gw`
  - Examples: `examples/*.gw`
- [ ] Fork `github-linguist/linguist`.
- [ ] Add `Goraw` entry into `lib/linguist/languages.yml`.
- [ ] Add sample files into `samples/Goraw/` (e.g. from `examples/tour.gw` and `examples/native_mem_main.gw`).
- [ ] Run `bundle exec rake test` to ensure all tests pass.
- [ ] Open Pull Request titled: `Add support for Goraw`.
