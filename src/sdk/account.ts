import oyl from "@oyl/sdk";

export function setupAccount(mnemonic: string, network: any) {
  const account = oyl.mnemonicToAccount({ mnemonic, opts: { network } });
  const keys = oyl.getWalletPrivateKeys({ mnemonic, opts: { network } });
  const signer = new oyl.Signer(network, {
    taprootPrivateKey: keys.taproot.privateKey,
    segwitPrivateKey: keys.nativeSegwit.privateKey,
    nestedSegwitPrivateKey: keys.nestedSegwit.privateKey,
    legacyPrivateKey: keys.legacy.privateKey,
  });

  return { account, signer };
}
