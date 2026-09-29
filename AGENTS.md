# Project workspace

- The canonical local checkout is `/Users/ncy/Projects/DesktopPet`.
- Run subsequent development commands from that directory. The old location may be a compatibility symlink; do not create a second checkout there.
- Read `README.md` and the relevant design documents in `docs/` before implementation. The project currently contains architecture and planning documents, not a runnable app.
- Keep imported character assets, model weights, local environment files, and credentials out of Git. Use distributable demo assets when adding examples.
- When a change needs the user's desktop validation, build a current local macOS `.app`, verify its bundle/signature, and provide its absolute path before requesting testing. Keep test assets and app bundles out of Git.
