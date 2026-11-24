import type { APIRoute } from 'astro';
import { agentPrompt } from '@/agent-prompt';

export const GET: APIRoute = () =>
  new Response(`${agentPrompt}\n`, {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
