// Images and voices for the editor preview, loaded on demand around the playhead and freed
// when far away (a decoded 2150×1210 image is ~10 MB; 8 GB laptops, docs/01-product.md).

import { api } from "./api";

interface Entry {
  image: ImageBitmap | null;
  audio: string | null;
  loading: boolean;
  failed: boolean;
}

export class PreviewAssets {
  private entries = new Map<string, Entry>();
  private disposed = false;

  constructor(
    private readonly projectId: string,
    private readonly audioMime: string,
    private readonly onLoaded: () => void,
  ) {}

  /** Starts loading what's missing; returns the entry (possibly still empty). */
  get(sceneId: string, wantImage: boolean, wantAudio: boolean): Entry {
    let e = this.entries.get(sceneId);
    if (!e) {
      e = { image: null, audio: null, loading: false, failed: false };
      this.entries.set(sceneId, e);
    }
    const missing = (wantImage && !e.image) || (wantAudio && !e.audio);
    if (missing && !e.loading && !e.failed) void this.load(sceneId, e, wantImage, wantAudio);
    return e;
  }

  /** Ready to play: everything the scene has is loaded (or failed — then shown as missing). */
  ready(sceneId: string, wantImage: boolean, wantAudio: boolean): boolean {
    const e = this.get(sceneId, wantImage, wantAudio);
    return e.failed || ((!wantImage || e.image !== null) && (!wantAudio || e.audio !== null));
  }

  /** Frees every scene not in `keep`. */
  retain(keep: Set<string>) {
    for (const [id, e] of this.entries) {
      if (keep.has(id) || e.loading) continue;
      e.image?.close();
      if (e.audio) URL.revokeObjectURL(e.audio);
      this.entries.delete(id);
    }
  }

  dispose() {
    this.disposed = true;
    this.retain(new Set());
  }

  private async load(sceneId: string, e: Entry, wantImage: boolean, wantAudio: boolean) {
    e.loading = true;
    try {
      const [img, audio] = await Promise.all([
        wantImage && !e.image ? api.sceneImageFull(this.projectId, sceneId) : null,
        wantAudio && !e.audio ? api.sceneVoiceAudio(this.projectId, sceneId) : null,
      ]);
      if (this.disposed) return;
      if (img) e.image = await createImageBitmap(new Blob([img]));
      if (audio) e.audio = URL.createObjectURL(new Blob([audio], { type: this.audioMime }));
    } catch {
      e.failed = true;
    } finally {
      e.loading = false;
      if (!this.disposed) this.onLoaded();
    }
  }
}
