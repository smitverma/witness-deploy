# Contributing to Witness

Thanks for helping improve Witness. By participating, you agree to use the project only for authorised security testing and to follow the repository's code of respectful collaboration.

## Workflow

1. Open an issue describing the problem or focused change.
2. Create a small branch from `main`.
3. Add implementation and tests; include a regression test for bug fixes.
4. Run the checks in [developer setup](../docs/developer-setup.md).
5. Open a pull request explaining behaviour, risk, test evidence, UI screenshots where relevant, and any migration impact.

## Style

- Rust: `cargo fmt`, warning-free Clippy, explicit errors instead of panics in runtime paths.
- Svelte/TypeScript: `npm run check`, accessible native controls, typed command payloads, and no unchecked `any`.
- Keep UI work usable at 1024×768, in dark/light and high-contrast modes.
- Preserve project compatibility and avoid loading large bodies on list views.

## Security reports

Do not publish vulnerabilities that expose users' captured traffic or CA keys. Report them privately to the maintainers with reproduction steps, impact, and a suggested mitigation if available.

## Commits

Use focused imperative commits such as `fix: preserve chunked trailers` or `feat: add log filtering`. Do not include generated build output, local projects, certificates, or secrets.
