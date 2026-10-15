import { Cpu, X } from "lucide-react";
import { useEffect, useState } from "react";

import { Button, IconButton } from "./Button";
import { useI18n } from "../i18n/context";
import { api } from "../lib/api";
import type { LocalStatus } from "../lib/types";

interface Props {
  /** Opens Settings (where the download runs with its progress). */
  onInstall: () => void;
}

/**
 * One-time card (08 §9): shown when the GPU can run local images, a fitting model profile
 * exists and none is installed yet. "Not now" hides it for good.
 */
export function LocalOfferCard({ onInstall }: Props) {
  const { t } = useI18n();
  const [status, setStatus] = useState<LocalStatus | null>(null);
  const [hidden, setHidden] = useState(false);

  useEffect(() => {
    api
      .localStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  if (!status || hidden) return null;
  const fitting = status.profiles.filter((p) => p.fits);
  const smallest = fitting.reduce<(typeof fitting)[number] | null>((a, p) => (a && a.total_mb <= p.total_mb ? a : p), null);
  if (!status.capable || status.prefs.offer_dismissed || !smallest || status.profiles.some((p) => p.installed)) return null;

  return (
    <section className="offer card" aria-label={t("local.offerTitle")}>
      <div className="offer__icon" aria-hidden="true">
        <Cpu size={18} strokeWidth={1.5} />
      </div>
      <div className="offer__body">
        <strong>{t("local.offerTitle")}</strong>
        <span className="t-small">{t("local.offerBody", { gb: (smallest.total_mb / 1024).toFixed(1) })}</span>
      </div>
      <div className="row">
        <Button variant="primary" onClick={onInstall}>
          {t("local.install")}
        </Button>
        <Button
          onClick={() => {
            setHidden(true);
            void api.localOfferDismiss();
          }}
        >
          {t("local.notNow")}
        </Button>
        <IconButton
          label={t("common.close")}
          icon={<X size={16} strokeWidth={1.5} />}
          onClick={() => {
            setHidden(true);
            void api.localOfferDismiss();
          }}
        />
      </div>
    </section>
  );
}
