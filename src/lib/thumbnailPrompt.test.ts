import { describe, expect, it } from "vitest";

import type { Project, Scene } from "./types";
import { buildThumbnailPrompt } from "./thumbnailPrompt";

function scene(id: string, imagePrompt: string, key: boolean): Scene {
  return { id, image_prompt: imagePrompt, key_scene: key } as Scene;
}

function project(scenes: Scene[]): Project {
  return {
    title: "How rice is milled",
    topic: "How a rice mill turns paddy into white rice",
    tone: "calm",
    style: { preset: "stickman", style_prompt: "simple hand-drawn stick figure doodle" },
    outline: { title: "From paddy to plate", hook: "Every grain loses its coat.", chapters: [], facts: [] },
    visual_bible: null,
    scenes,
  } as unknown as Project;
}

describe("buildThumbnailPrompt", () => {
  it("never asks for text and says so explicitly", () => {
    const p = buildThumbnailPrompt(project([scene("1", "a husking machine pours rice", true)]));
    expect(p).toContain("ABSOLUTELY NO TEXT");
    expect(p).toContain("16:9");
    expect(p).toContain("never write it in the image");
  });

  it("uses the first key scene as the main idea, else the first scene", () => {
    const keyed = buildThumbnailPrompt(
      project([scene("1", "plain scene", false), scene("2", "huge rice mill at sunrise", true)]),
    );
    expect(keyed).toContain("huge rice mill at sunrise");
    const unkeyed = buildThumbnailPrompt(project([scene("1", "plain scene", false)]));
    expect(unkeyed).toContain("plain scene");
  });

  it("works with no scenes yet", () => {
    const p = buildThumbnailPrompt(project([]));
    expect(p).toContain("From paddy to plate");
    expect(p).not.toContain("Main idea");
  });
});
