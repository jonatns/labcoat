export default {
  network: "regtest",
  // Optional: pin a mnemonic for deterministic dev addresses.
  // The wallet keystore lives at .labcoat/wallet.json (gitignored).
  mnemonic: process.env.MNEMONIC,
};
