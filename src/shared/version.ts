/**
 * DevHQ's version, as the window shows it. The same number is written in package.json, in
 * src-tauri/Cargo.toml and in src-tauri/tauri.conf.json, which the installer and the executable's
 * version resource take it from: a test holds the four together. It changes when an installer is
 * built to be installed, not with every change: the commit each build writes after it in its
 * version resource («0.2.0+3f9c2ab01d4e») is what tells one build from another.
 */
export const devhqVersion = '0.2.0'
