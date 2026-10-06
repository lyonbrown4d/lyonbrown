import { controlPlaneError } from "@lyonbrown/local-control-plane/internal/errors";

function skipWhitespace(text: string, start: number): number {
  let index = start;
  while (index < text.length && /\s/u.test(text[index] ?? "")) index += 1;
  return index;
}

function readStringEnd(text: string, start: number): number {
  let escaped = false;
  for (let index = start + 1; index < text.length; index += 1) {
    const character = text[index];
    if (escaped) {
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === '"') {
      return index + 1;
    }
  }
  throw controlPlaneError("INVALID_DESCRIPTOR");
}

function skipComposite(text: string, start: number): number {
  const pairs: Record<string, string> = { "{": "}", "[": "]" };
  const stack = [pairs[text[start] ?? ""]];
  let index = start + 1;
  while (index < text.length && stack.length > 0) {
    const character = text[index] ?? "";
    if (character === '"') {
      index = readStringEnd(text, index);
      continue;
    }
    if (character === "{" || character === "[") stack.push(pairs[character]);
    else if (character === stack.at(-1)) stack.pop();
    index += 1;
  }
  if (stack.length > 0) throw controlPlaneError("INVALID_DESCRIPTOR");
  return index;
}

function skipValue(text: string, start: number): number {
  const character = text[start];
  if (character === '"') return readStringEnd(text, start);
  if (character === "{" || character === "[") return skipComposite(text, start);
  let index = start;
  while (index < text.length && text[index] !== "," && text[index] !== "}") index += 1;
  return index;
}

export function rejectDuplicateTopLevelKeys(text: string): void {
  let index = skipWhitespace(text, 0);
  if (text[index] !== "{") return;
  index = skipWhitespace(text, index + 1);
  const keys = new Set<string>();

  while (index < text.length && text[index] !== "}") {
    if (text[index] !== '"') throw controlPlaneError("INVALID_DESCRIPTOR");
    const keyEnd = readStringEnd(text, index);
    let key: string;
    try {
      key = JSON.parse(text.slice(index, keyEnd)) as string;
    } catch {
      throw controlPlaneError("INVALID_DESCRIPTOR");
    }
    if (keys.has(key)) throw controlPlaneError("DUPLICATE_DESCRIPTOR_KEY");
    keys.add(key);

    index = skipWhitespace(text, keyEnd);
    if (text[index] !== ":") throw controlPlaneError("INVALID_DESCRIPTOR");
    index = skipWhitespace(text, index + 1);
    index = skipWhitespace(text, skipValue(text, index));
    if (text[index] === ",") index = skipWhitespace(text, index + 1);
    else if (text[index] !== "}") throw controlPlaneError("INVALID_DESCRIPTOR");
  }
}
