# Isomer

<br />
<div align="center">
  <img src="src-tauri/icons/isomer-logo.svg" alt="Isomer Logo" width="128" height="128">
</div>
<br />

**One-click Alkanes development environment.**

Isomer is a desktop application powered by [Tauri](https://tauri.app/), [React](https://react.dev/), and [Rust](https://www.rust-lang.org/) that simplifies managing a local Bitcoin Regtest environment with full Alkanes support.

## Features

- **Service Management**: Easily start/stop Bitcoin Core, Metashrew, Memshrew, Esplora, and Alkanes JSON-RPC.
- **Auto-Configuration**: Automatically handles service dependencies and configuration.
- **Binary Management**: Downloads and manages necessary binaries for your platform.
- **Development Tools**: Includes a faucet, block generator, and integrated logs panel.

## Prerequisites

- **Rust**: Ensure you have the latest stable Rust toolchain installed: [rustup.rs](https://rustup.rs/)
- **Node.js**: LTS version recommended.
- **pnpm**: Package manager.

## Getting Started

1.  **Install Dependencies**

    ```bash
    pnpm install
    ```

2.  **Run in Development Mode**

    Start the Tauri application in development mode with hot-reloading:

    ```bash
    pnpm tauri dev
    ```

3.  **Build for Production**

    Create a production build of the application:

    ```bash
    pnpm tauri build
    ```

## Project Structure

- `src/`: React frontend source code.
- `src-tauri/`: Rust backend source code (Tauri).
- `src-tauri/src/binary_manager.rs`: Logic for downloading and verifying external binaries.
- `src-tauri/src/process_manager.rs`: Logic for managing service processes.

## License

MIT
