export function toAlkanesId({ block, tx, }) {
    return `${Number(block)}:${Number(tx)}`;
}
