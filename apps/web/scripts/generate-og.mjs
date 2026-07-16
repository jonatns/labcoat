import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import sharp from 'sharp';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
await mkdir(path.join(root, 'public'), { recursive: true });
await sharp(path.join(root, 'src/assets/og-card.svg'))
  .png({ compressionLevel: 9, palette: true })
  .toFile(path.join(root, 'public/og.png'));
