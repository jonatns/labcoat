import type { APIRoute } from 'astro';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const installer = path.resolve(process.cwd(), '../../install-labcoat.sh');

export const GET: APIRoute = async () =>
  new Response(await readFile(installer, 'utf8'), {
    headers: {
      'Content-Type': 'text/x-shellscript; charset=utf-8',
      'Content-Disposition': 'inline; filename="install-labcoat.sh"',
    },
  });
