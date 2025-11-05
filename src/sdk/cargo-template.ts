export const cargoTemplate = `[package]
name = "alkanes-contract"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
alkanes-runtime = { git = "https://github.com/kungfuflex/alkanes-rs" }
alkanes-support = { git = "https://github.com/kungfuflex/alkanes-rs" }
metashrew-support = { git = "https://github.com/sandshrewmetaprotocols/metashrew" }
anyhow = "1.0"
`;
