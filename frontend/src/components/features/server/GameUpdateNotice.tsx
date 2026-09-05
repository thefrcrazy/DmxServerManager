import { PackageCheck, RefreshCw, Square, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import type { GameUpdateStatus } from "@/schemas/api";

interface GameUpdateNoticeProps {
    status: GameUpdateStatus;
    running: boolean;
    busy: boolean;
    checking: boolean;
    /** Le profil sait installer une mise à jour depuis le panneau. */
    canInstall: boolean;
    canUpdateGame: boolean;
    canStop: boolean;
    /** Une mise à jour n'est applicable qu'à l'arrêt et hors relance programmée. */
    stoppedOnPurpose: boolean;
    onCheck: () => void;
    onUpdate: () => void;
    onStop: () => void;
}

function reference(status: GameUpdateStatus, kind: "installed" | "available"): string | null {
    return kind === "installed"
        ? status.installed_version ?? status.installed_build ?? null
        : status.available_version ?? status.available_build ?? null;
}

/**
 * Verdict de mise à jour, sous une forme unique pour les trois états.
 *
 * Chaque état avait auparavant sa propre mise en forme — un encadré accentué,
 * un encadré orange, et une ligne de texte nue posée au-dessus des boutons
 * d'action. Les trois disaient pourtant la même chose sur le même sujet, et le
 * passage de l'un à l'autre déplaçait la mise en page sous les yeux.
 */
export default function GameUpdateNotice({
    status,
    running,
    busy,
    checking,
    canInstall,
    canUpdateGame,
    canStop,
    stoppedOnPurpose,
    onCheck,
    onUpdate,
    onStop,
}: GameUpdateNoticeProps) {
    const { t } = useLanguage();
    if (status.state === "not_installed") return null;

    const installed = reference(status, "installed");
    const available = reference(status, "available");
    const checkedAt = Number.isNaN(Date.parse(status.checked_at))
        ? null
        : new Date(status.checked_at).toLocaleTimeString(undefined, {
            hour: "2-digit",
            minute: "2-digit",
        });

    const tone = status.state === "update_available"
        ? "available"
        : status.state === "check_failed"
        ? "failed"
        : "current";

    let icon: ReactNode = <PackageCheck size={18} aria-hidden="true" />;
    let title = t("server_detail.update_up_to_date");
    let detail: ReactNode = null;

    if (status.state === "update_available") {
        title = t("server_detail.update_available_title");
        detail = (
            <span className="update-notice__versions">
                <span className="update-notice__version">{installed ?? "—"}</span>
                <span aria-hidden="true">→</span>
                <span className="update-notice__version update-notice__version--next">
                    {available ?? "—"}
                </span>
            </span>
        );
    } else if (status.state === "check_failed") {
        icon = <TriangleAlert size={18} aria-hidden="true" />;
        title = t("server_detail.update_check_failed_title");
        detail = <span>{t("server_detail.update_check_failed_detail")}</span>;
    }
    // À jour : la version installée figure déjà dans la vignette au-dessus, la
    // répéter ici n'ajoutait rien et donnait deux fois la même valeur à lire.

    return (
        <div className={`update-notice update-notice--${tone}`} role="status">
            {icon}
            <div className="update-notice__body">
                <strong>{title}</strong>
                {detail}
                {status.state === "update_available" && running && (
                    <small>{t("server_detail.update_requires_stop")}</small>
                )}
                {checkedAt && (
                    <small className="update-notice__checked">
                        {t("server_detail.update_checked_at").replace("{{time}}", checkedAt)}
                    </small>
                )}
            </div>
            <div className="update-notice__actions">
                {canUpdateGame && (
                    <Button
                        variant="ghost"
                        size="sm"
                        isLoading={checking}
                        onClick={onCheck}
                        icon={<RefreshCw size={15} />}
                    >
                        {t("server_detail.update_check_now")}
                    </Button>
                )}
                {status.state === "update_available" && canInstall && canUpdateGame && !running
                    && stoppedOnPurpose && (
                    <Button
                        variant="primary"
                        size="sm"
                        onClick={onUpdate}
                        disabled={busy}
                        icon={<RefreshCw size={15} />}
                    >
                        {t("server_detail.update_game")}
                    </Button>
                )}
                {status.state === "update_available" && canInstall && canUpdateGame && running
                    && canStop && (
                    <Button
                        variant="secondary"
                        size="sm"
                        onClick={onStop}
                        disabled={busy}
                        icon={<Square size={15} />}
                    >
                        {t("server_detail.update_stop_to_apply")}
                    </Button>
                )}
            </div>
        </div>
    );
}
