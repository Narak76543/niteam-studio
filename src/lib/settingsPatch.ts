// The fields of `next` that differ from `base`, nested objects compared field by field.
// Settings sections send only this, so a section holding an older copy cannot undo a change
// made elsewhere (a finished download, another section). Arrays are replaced whole.

type Json = string | number | boolean | null | Json[] | { [key: string]: Json };

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export function settingsPatch(base: unknown, next: unknown): Record<string, Json> {
  const patch: Record<string, Json> = {};
  if (!isObject(base) || !isObject(next)) return patch;
  for (const [key, value] of Object.entries(next)) {
    const old = base[key];
    if (isObject(old) && isObject(value)) {
      const inner = settingsPatch(old, value);
      if (Object.keys(inner).length > 0) patch[key] = inner;
    } else if (JSON.stringify(old) !== JSON.stringify(value)) {
      patch[key] = (value === undefined ? null : value) as Json;
    }
  }
  return patch;
}
