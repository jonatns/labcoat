export function toAlkanesId({
  block,
  tx,
}: {
  block: number;
  tx: number;
}): string {
  return `${Number(block)}:${Number(tx)}`;
}
