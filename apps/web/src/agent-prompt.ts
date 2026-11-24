// The one prompt Labcoat asks people to hand to a coding agent. Every place
// that shows or serves it reads this module, and the build fails if the
// copies in the README and the docs drift from it (scripts/verify-build.mjs).
//
// The source file holds one paragraph per line with a blank line between
// them. Nothing is hard-wrapped: the page wraps the text with CSS, and what
// reaches the clipboard has no line breaks inside a sentence.
import source from './agent-prompt.txt?raw';

export const agentPrompt = source.trim();
export const agentPromptParagraphs = agentPrompt.split(/\n\s*\n/);
