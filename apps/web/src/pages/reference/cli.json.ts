import type { APIRoute } from 'astro';
import reference from '../../generated/cli-reference.json';

export const GET: APIRoute = () =>
  new Response(JSON.stringify(reference, null, 2), {
    headers: { 'Content-Type': 'application/json; charset=utf-8' },
  });
