# Publication checklist

This repository is ready for local development and review. Marketplace publication is not complete.

1. The public source repository is [xandertaylorx/java-hotswap](https://github.com/xandertaylorx/java-hotswap). Keep the repository URL and maintainer identity in `extension.toml` up to date. The extension name is **Java HotSwap** and its debugger-specific ID is `java-hotswap-debugger`.
2. Run CI and manually test the exact submission commit in Zed. Verify Run/Stop/output, interaction with the existing Java extension, task installation, and optional JDWP attach. Manual in-Zed testing is an official publication prerequisite.
3. Ask Zed maintainers whether a launch/output adapter without stepping is acceptable as a debugger extension. This is deliberately implemented as a supported DAP server, but registry acceptance is discretionary; do not claim approval. Contributing this launch integration to the Java extension may be an appropriate alternative.
4. Review [publishing prerequisites](https://zed.dev/docs/extensions/publishing/prerequisites) and [license rules](https://zed.dev/docs/extensions/publishing/licenses). MIT is included. The extension finds its companion via the SDK rather than bundling a native adapter in WASM.
5. Build/checksum candidates using the release workflow; test each binary on its actual architecture. The Intel build is a cross-compile, not an Intel runtime test. The workflow uploads artifacts and never automatically publishes a GitHub release.
6. Follow the [official publishing guide](https://zed.dev/docs/extensions/publishing/publishing-guide): fork `zed-industries/extensions`, add this repo under `extensions/java-hotswap-debugger` as a Git submodule, add the entry to `extensions.toml` with the version and submodule path, and open a PR. Follow the guide's current commands and registry layout.
7. Keep the one-time companion installation requirement prominent. A later release may implement OS/architecture-specific verified binary downloads through supported SDK APIs once real release URLs exist.

Do not submit placeholder repository metadata, claim unrun platform tests passed, or describe the extension as a full Java debugger. Do not publish experimental Windows binaries as production-supported until process-tree cleanup and wrapper tests pass.
