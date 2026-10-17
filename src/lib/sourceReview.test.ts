import { describe, expect, it } from "vitest";

import { moveKeyPoint, notesForPage, pageRanges, toReviewEdit, updateItem } from "./sourceReview";
import type { SourceItem, SourceSummary } from "./types";

const item = (id: string, pages: number[] = [1]): SourceItem => ({
  id,
  term: "",
  text: id,
  pages,
  selected: true,
  note: "",
});

const summary: SourceSummary = {
  title: "Rice",
  authors: "",
  year: "",
  key_points: [item("k1"), item("k2"), item("k3")],
  facts: [item("f1")],
  terms: [item("t1")],
};

describe("source review", () => {
  it("reorders key points and keeps the ends in place", () => {
    expect(moveKeyPoint(summary, "k3", -1).key_points.map((k) => k.id)).toEqual(["k1", "k3", "k2"]);
    expect(moveKeyPoint(summary, "k1", -1)).toBe(summary);
    expect(moveKeyPoint(summary, "k3", 1)).toBe(summary);
  });

  it("ticks and notes any item, and builds the edit", () => {
    const s = updateItem(updateItem(summary, "f1", { selected: false }), "k2", { note: "keep short" });
    const edit = toReviewEdit(moveKeyPoint(s, "k2", -1));
    expect(edit.key_point_order).toEqual(["k2", "k1", "k3"]);
    expect(edit.items.find((i) => i.id === "f1")?.selected).toBe(false);
    expect(edit.items.find((i) => i.id === "k2")?.note).toBe("keep short");
    expect(edit.items).toHaveLength(5);
  });

  it("formats page ranges and finds notes by page", () => {
    expect(pageRanges([5, 3, 4, 8, 8])).toBe("3–5, 8");
    expect(pageRanges([2])).toBe("2");
    expect(pageRanges([])).toBe("");
    const sections = [
      { heading: "A", notes: "a", pages: [1, 2] },
      { heading: "B", notes: "b", pages: [3] },
    ];
    expect(notesForPage(sections, 2).map((s) => s.heading)).toEqual(["A"]);
  });
});
