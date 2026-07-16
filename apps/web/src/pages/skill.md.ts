import type { APIRoute } from 'astro';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const source = path.resolve(process.cwd(), '../../skills/SKILL.md');

export const GET: APIRoute = async () =>
  new Response(await readFile(source, 'utf8'), {
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  });
