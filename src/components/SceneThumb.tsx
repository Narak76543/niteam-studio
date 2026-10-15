import { ImageIcon } from "lucide-react";
import { useEffect, useState } from "react";

import { api } from "../lib/api";

interface Props {
  projectId: string;
  sceneId: string;
  /** Changes whenever the image file may have changed (null = no image yet). */
  version: string | null;
  alt: string;
}

/** 384×216 `thumb.jpg` of a scene, or a placeholder. */
export function SceneThumb({ projectId, sceneId, version, alt }: Props) {
  const [loaded, setLoaded] = useState<{ version: string; url: string } | null>(null);

  useEffect(() => {
    if (version === null) return;
    let url: string | null = null;
    let alive = true;
    api
      .sceneImageThumb(projectId, sceneId)
      .then((bytes) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
        setLoaded({ version, url });
      })
      .catch(() => {
        if (alive) setLoaded(null);
      });
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [projectId, sceneId, version]);

  const url = loaded && loaded.version === version ? loaded.url : null;
  return (
    <div className="thumb">
      {url ? <img className="thumb__img" src={url} alt={alt} /> : <ImageIcon size={20} strokeWidth={1.5} aria-hidden="true" />}
    </div>
  );
}
