// Light / dark mode (docs/06-ui-design.md §2): sets data-theme on <html>, which switches the
// tokens in tokens.css. `null` follows the Windows app mode and updates when it changes.

import { useEffect, useState } from "react";

import type { Theme } from "./types";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function systemTheme(): Theme {
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

export function useTheme(choice: Theme | null): Theme {
  const [system, setSystem] = useState<Theme>(systemTheme);

  useEffect(() => {
    const media = window.matchMedia(DARK_QUERY);
    const onChange = () => setSystem(media.matches ? "dark" : "light");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const theme = choice ?? system;
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  return theme;
}
