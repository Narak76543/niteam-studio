import { Clapperboard, Palette, Shapes } from "lucide-react";

import type { PresetId } from "../lib/presets";

export function PresetIcon({ id, size = 24 }: { id: PresetId; size?: number }) {
  const props = { size, strokeWidth: 1.5, "aria-hidden": true } as const;
  switch (id) {
    case "cinematic_realistic":
      return <Clapperboard {...props} />;
    case "flat_2d":
      return <Shapes {...props} />;
    case "storybook_watercolor":
      return <Palette {...props} />;
  }
}
