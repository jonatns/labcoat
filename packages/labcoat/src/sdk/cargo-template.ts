// alkanes-rs is pinned to a commit on its `develop` branch — never `main`,
// never a moving branch ref. Keep in sync with TOOLCHAIN.md when upgrading.
const ALKANES_RS_REV = "5b7f43567b828d0bb7b8907ce78fa0242943c54d";
// metashrew rev matches alkanes-rs's own Cargo.lock at the pinned commit.
const METASHREW_REV = "eca790ca1eeddc7cdac201b741637b8f18234924";

export const cargoTemplate = `[package]
name = "alkanes-contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
alkanes-runtime = { git = "https://github.com/kungfuflex/alkanes-rs", rev = "${ALKANES_RS_REV}" }
alkanes-support = { git = "https://github.com/kungfuflex/alkanes-rs", rev = "${ALKANES_RS_REV}" }
metashrew-support = { git = "https://github.com/sandshrewmetaprotocols/metashrew", rev = "${METASHREW_REV}" }
anyhow = "1.0"
`;
