/** Style preset ids; must match `pipeline::presets::PRESETS` in Rust. */
export const PRESET_IDS = ["stickman", "storybook_watercolor", "cinematic_realistic", "flat_2d"] as const;

export type PresetId = (typeof PRESET_IDS)[number];

export function isPresetId(id: string): id is PresetId {
  return (PRESET_IDS as readonly string[]).includes(id);
}

/** i18n key of a preset label. */
export function presetKey(id: string): `preset.${PresetId}` {
  return isPresetId(id) ? `preset.${id}` : "preset.storybook_watercolor";
}
