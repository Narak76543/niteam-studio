import { type RefObject, useEffect, useState } from "react";

interface Props {
  /** Container whose `h2` headings become the jump targets. */
  container: RefObject<HTMLElement | null>;
  label: string;
}

/**
 * Sticky list of jump links, one per `h2` in `container` (Settings page). Built from the
 * rendered headings, so sections that appear later are picked up automatically.
 */
export function SectionJumpBar({ container, label }: Props) {
  const [headings, setHeadings] = useState<HTMLElement[]>([]);
  const [active, setActive] = useState(0);

  useEffect(() => {
    const root = container.current;
    if (!root) return;
    const collect = () => {
      const next = [...root.querySelectorAll<HTMLElement>("h2")].filter((h) => h.textContent?.trim());
      setHeadings((prev) => (prev.length === next.length && prev.every((h, i) => h === next[i]) ? prev : next));
    };
    collect();
    const mo = new MutationObserver(collect);
    mo.observe(root, { childList: true, subtree: true });
    return () => mo.disconnect();
  }, [container]);

  useEffect(() => {
    if (headings.length === 0) return;
    // The heading nearest the top of the visible area is the current section.
    const io = new IntersectionObserver(
      (entries) => {
        const top = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
        if (top) setActive(headings.indexOf(top.target as HTMLElement));
      },
      { rootMargin: "0px 0px -70% 0px" },
    );
    for (const h of headings) io.observe(h);
    return () => io.disconnect();
  }, [headings]);

  if (headings.length < 2) return null;
  return (
    <nav className="jump-bar" aria-label={label}>
      {headings.map((h, i) => (
        <button
          key={i}
          className="jump-bar__item"
          aria-current={i === active ? "true" : undefined}
          onClick={() => {
            setActive(i);
            h.scrollIntoView({ behavior: "smooth", block: "start" });
          }}
        >
          {h.textContent}
        </button>
      ))}
    </nav>
  );
}
