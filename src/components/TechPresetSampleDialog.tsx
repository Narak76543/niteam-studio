import { useEffect, useRef } from "react";

import { Button } from "./Button";
import { useI18n } from "../i18n/context";
import codeSample from "../assets/preset-samples/tech-code.jpg";
import diagramSample from "../assets/preset-samples/tech-diagram.jpg";
import photoSample from "../assets/preset-samples/tech-photo.jpg";

interface Props {
  onClose: () => void;
  onStart: () => void;
}

const samples = [
  { src: photoSample, title: "projects.sample.photo", alt: "projects.sample.photoAlt" },
  { src: diagramSample, title: "projects.sample.diagram", alt: "projects.sample.diagramAlt" },
  { src: codeSample, title: "projects.sample.code", alt: "projects.sample.codeAlt" },
] as const;

export function TechPresetSampleDialog({ onClose, onStart }: Props) {
  const { t } = useI18n();
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="dialog dialog--preset-sample" role="dialog" aria-modal="true" aria-labelledby="preset-sample-title">
        <div className="preset-sample__heading">
          <div>
            <h2 id="preset-sample-title" className="t-h2">
              {t("projects.sample.title")}
            </h2>
            <p className="t-small t-muted">{t("projects.sample.body")}</p>
          </div>
          <button ref={closeRef} className="preset-sample__close" onClick={onClose} aria-label={t("common.close")}>
            {t("common.close")}
          </button>
        </div>
        <div className="preset-sample__grid">
          {samples.map((sample) => (
            <figure className="preset-sample__item" key={sample.title}>
              <img src={sample.src} alt={t(sample.alt)} />
              <figcaption className="t-small">{t(sample.title)}</figcaption>
            </figure>
          ))}
        </div>
        <p className="preset-sample__credit t-micro">
          {t("projects.sample.photoCredit")}{" "}
          <a href="https://commons.wikimedia.org/wiki/File:Technician_with_laptop_working_on_server_rack_at_NERSC.jpg" target="_blank" rel="noreferrer">
            {t("projects.sample.photoSource")}
          </a>
        </p>
        <div className="dialog__actions">
          <Button onClick={onClose}>{t("common.close")}</Button>
          <Button variant="primary" onClick={onStart}>{t("projects.sample.start")}</Button>
        </div>
      </div>
    </div>
  );
}
