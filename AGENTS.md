# Project workspace

- Run development commands from the active checkout's repository root. Never put machine-specific checkout paths into shared instructions.
- Read `README.md` and relevant design documents in `docs/` before implementation. The repository includes working macOS and Windows development applications.
- Keep imported character assets, model weights, local environment files, and credentials out of Git. Use distributable demo assets when adding examples.
- When a change needs desktop validation, build a current local package for the target platform and provide its absolute path. Keep test assets and application bundles out of Git. Document public build steps with relative paths or environment variables; omit private paths, proxy configuration, secrets and raw user settings.
