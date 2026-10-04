import { stripTerminalSequences } from "@earendil-works/pi-tui";

export function safeTerminalText(value: string): string {
  let safe = "";
  for (const character of stripTerminalSequences(value)) {
    const code = character.charCodeAt(0);
    if (code === 9 || code === 10 || code === 13 || (code >= 32 && (code < 127 || code > 159))) {
      safe += character;
    }
  }
  return safe;
}
