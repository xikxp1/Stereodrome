# Task Completion Checklist

When a development task is completed, run through this checklist:

## 1. Code Quality

### TypeScript/Svelte
- [ ] Run `bun run check` to verify type correctness
- [ ] Verify Svelte 5 runes syntax is used correctly
- [ ] Check that component imports and exports are correct

### Rust
- [ ] Run `cargo fmt` in `src-tauri/` to format code
- [ ] Run `cargo clippy` in `src-tauri/` and address warnings
- [ ] Run `cargo test` if tests exist
- [ ] Ensure no compiler warnings with `cargo build`

## 2. Testing

### Manual Testing
- [ ] Run `bun run tauri dev` to test changes in development mode
- [ ] Verify the feature/fix works as expected in the running app
- [ ] Test on the target platform (macOS in this case)
- [ ] Check for console errors in both frontend and backend

### Automated Testing (when applicable)
- [ ] Add/update unit tests for Rust code
- [ ] Add/update tests for Svelte components (if testing framework is added)

## 3. Build Verification

- [ ] Run `bun run build` to ensure frontend builds successfully
- [ ] Run `cargo build --release` in `src-tauri/` to ensure Rust builds
- [ ] Optionally run `bun run tauri build` to create production bundle (for major changes)

## 4. Documentation

- [ ] Update README.md if functionality changed
- [ ] Add/update code comments for complex logic
- [ ] Update memory files if project structure or conventions changed

## 5. Version Control

- [ ] Review all changed files
- [ ] Ensure no sensitive data or debug code is committed
- [ ] Stage relevant changes: `git add <files>`
- [ ] Commit with descriptive message: `git commit -m "description"`
- [ ] Push if appropriate: `git push`

## Quick Checklist for Small Changes

For minor bug fixes or small features:
1. `bun run check` (for frontend changes)
2. `cargo fmt && cargo clippy` (for Rust changes)
3. `bun run tauri dev` (verify it works)
4. Commit changes

## When to Skip Steps

- **Formatting**: Skip if only modifying configuration files
- **Build verification**: Can skip production build for minor changes (dev testing is enough)
- **Testing**: Skip manual testing if change is trivial (e.g., typo fix in comment)
