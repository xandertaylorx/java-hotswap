# Contributing

Use the commands in README. Changes to the agent must retain Java 17 bytecode compatibility. Changes to Zed integration must compile against the pinned SDK and use supported host callbacks.

Add a real end-to-end fixture when expanding build-system or launch support. Verify unchanged JVM PID for reloads, rejected structural changes, compiler-failure recovery, and child cleanup. Do not infer platform support from a successful cross-compile. Keep publication blockers and actual local/CI results explicit.

No generated build outputs belong in commits. Cargo.lock files are committed for reproducible builds. Release binaries embed the agent and should be checksummed. Never add fabricated release URLs to automatic download code.
