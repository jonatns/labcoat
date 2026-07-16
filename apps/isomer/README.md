> [!NOTE]
> **Maintenance mode.** The Isomer desktop app is a thin UI over the shared
> `isomer-core` engine; the [`labcoat` CLI](../../README.md) is the flagship
> surface (`labcoat up` boots this exact stack headless). The app keeps
> compiling in CI, but new features land in the CLI first and app releases
> are tagged on demand.

<br />
<div align="center">
  <img src="src-tauri/icons/isomer-logo.svg" alt="Isomer Logo" width="128" height="128">
  <h1>Isomer</h1>
  <p><strong>One-click Alkanes development environment</strong></p>
  <br />

  [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
  [![Built with Tauri](https://img.shields.io/badge/Built%20with-Tauri-ffc131?logo=tauri)](https://tauri.app)
  [![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-dea584?logo=rust)](https://www.rust-lang.org)
</div>

<br />

<div align="center">
  <img src="assets/dashboard.png" alt="Isomer Dashboard" width="700">
</div>

<br />

**Isomer** is a desktop application that simplifies managing a local Bitcoin Regtest environment with full [Alkanes](https://github.com/kungfuflex/alkanes) metaprotocol support. Built with [Tauri](https://tauri.app/), [React](https://react.dev/), and [Rust](https://www.rust-lang.org/).

---

## ✨ Features

| Feature                   | Description                                                              |
| ------------------------- | ------------------------------------------------------------------------ |
| 🚀 **One-Click Launch**   | Start your entire Alkanes development stack with a single click          |
| ⚡ **JSON RPC Server**    | Prominent RPC endpoint display with copy-to-clipboard (Ganache-style)    |
| 🔧 **Service Management** | Easily control Bitcoin Core, Metashrew, Esplora, Ord, and JSON RPC       |
| 🔍 **Espo Explorer**      | Built-in block explorer with Alkanes trace visualization                 |
| 💰 **Faucet & Mining**    | Fund addresses and mine blocks directly from the UI                      |
| 📦 **Binary Management**  | Automatically downloads and verifies required binaries for your platform |
| 📋 **Integrated Logs**    | Real-time log streaming for all managed services                         |

---

## 🖥️ Managed Services

Isomer orchestrates the following services with proper dependency ordering:

```
┌─────────────────────────────────────────────────────────────┐
│                      Isomer Dashboard                       │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ Bitcoin     │  │ Metashrew   │  │ JSON RPC            │  │
│  │ Core        │──│ (Indexer)   │──│ (API Gateway)       │  │
│  │ (Regtest)   │  │             │  │                     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ Esplora     │  │ Ord         │  │ Espo Explorer       │  │
│  │ (Electrum)  │  │ (Ordinals)  │  │ (Block Explorer)    │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 🚀 Quick Install

### macOS / Linux / WSL

```bash
curl -sSf https://raw.githubusercontent.com/jonatns/isomer/main/install.sh | bash
```

### Windows

Download the latest `.msi` installer from [Releases](https://github.com/jonatns/isomer/releases).

---

## 🚀 Getting Started

### Prerequisites

- **Rust** — Latest stable toolchain: [rustup.rs](https://rustup.rs/)
- **Node.js** — LTS version recommended
- **pnpm** — Package manager: `npm i -g pnpm`

### Installation

```bash
# Clone the repository
git clone https://github.com/jonatns/isomer.git
cd isomer

# Install dependencies
pnpm install

# Build the application
pnpm tauri build
```

The built application will be available in `src-tauri/target/release/bundle/`.

### Development

For contributors who want to work on Isomer:

```bash
pnpm tauri dev
```

See [RELEASING.md](RELEASING.md) for details on the two-stage release process (binaries and app).

---

## 📁 Project Structure

```
isomer/
├── src/                    # React frontend
│   ├── components/         # UI components
│   └── index.css           # Global styles
├── src-tauri/              # Rust backend (Tauri)
│   ├── src/
│   │   ├── binary_manager.rs   # Binary download & verification
│   │   ├── process_manager.rs  # Service lifecycle management
│   │   └── commands.rs         # Tauri command handlers
│   └── icons/                  # Application icons
└── assets/                 # Documentation assets
```

---

## 🔌 Default Ports

| Service          | Port    |
| ---------------- | ------- |
| Bitcoin RPC      | `18443` |
| Metashrew RPC    | `8080`  |
| JSON RPC         | `18888` |
| Esplora Electrum | `50001` |
| Esplora HTTP     | `3002`  |
| Espo Explorer    | `8081`  |
| Ord              | `3001`  |

---

## 📜 License

[MIT](LICENSE)

---

## ❤️ Support

If you find this project useful, you can support the developer by donating to:

`bc1q3w72ctyxh5thnxmasaexh0yymga9pf8n9aaz2l`

---

<div align="center">
  <sub>Built with ⚗️ for the Alkanes ecosystem</sub>
</div>
