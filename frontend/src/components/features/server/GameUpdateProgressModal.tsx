import { Check, Loader2, TriangleAlert, X } from "lucide-react";
import SafeAnsi from "@/components/shared/SafeAnsi";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useFocusTrap } from "@/hooks";
import type { Job } from "@/schemas/api";

/**
 * Étapes communes à tous les installeurs.
 *
 * Les jalons de progression diffèrent d'un jeu à l'autre — SteamCMD, le
 * téléchargeur Hytale et les archives ne rapportent pas les mêmes pourcentages —
 * mais tous franchissent 10 puis 80 aux mêmes moments : entrée en phase de
 * récupération, puis bascule du répertoire validé. Les bornes sont donc tenues
 * larges plutôt que nommées finement à tort.
 */
const PHASES: ReadonlyArray<{ id: string; until: number }> = [
    { id: "prepare", until: 10 },
    { id: "download", until: 80 },
    { id: "apply", until: 100 },
];

function phaseIndex(progress: number): number {
    const index = PHASES.findIndex((phase) => progress < phase.until);
    return index === -1 ? PHASES.length : index;
}

interface GameUpdateProgressModalProps {
    serverName: string;
    /** Version quittée, telle qu'elle était avant le lancement. */
    fromVersion: string | null;
    /** Version visée, quand le fournisseur l'a annoncée. */
    toVersion: string | null;
    job: Job | null;
    /** Dernière ligne du journal d'installation, pour montrer que ça avance. */
    latestLine: string | null;
    onClose: () => void;
    onOpenTerminal: () => void;
}

export default function GameUpdateProgressModal({
    serverName,
    fromVersion,
    toVersion,
    job,
    latestLine,
    onClose,
    onOpenTerminal,
}: GameUpdateProgressModalProps) {
    const { t } = useLanguage();
    const { containerRef, onKeyDown } = useFocusTrap<HTMLDivElement>({ onEscape: onClose });

    const progress = job?.progress ?? 0;
    const failed = job?.state === "failed" || job?.state === "cancelled";
    const current = phaseIndex(progress);

    return (
        <div className="dialog-overlay">
            <div
                ref={containerRef}
                className="dialog update-progress"
                role="dialog"
                aria-modal="true"
                aria-labelledby="update-progress-title"
                tabIndex={-1}
                onKeyDown={onKeyDown}
            >
                <div className="update-progress__head">
                    <h2 id="update-progress-title">
                        {t("server_detail.update_progress.title").replace("{{name}}", serverName)}
                    </h2>
                    <Button
                        variant="ghost"
                        size="icon"
                        aria-label={t("common.close")}
                        onClick={onClose}
                    >
                        <X size={16} />
                    </Button>
                </div>

                {/* L'écart de versions est la première chose à lire : c'est lui qui
                    dit ce que l'opération est en train de changer. La cible n'est
                    pas toujours connue — une réinstallation à version choisie n'en
                    a pas, et une page ouverte alors que l'opération courait déjà
                    l'ignore : mieux vaut alors n'afficher que le point de départ
                    plutôt qu'une flèche pointant vers un tiret. */}
                {(fromVersion || toVersion) && (
                    <p className="update-progress__versions">
                        {fromVersion && (
                            <span className="update-progress__version">{fromVersion}</span>
                        )}
                        {fromVersion && toVersion && <span aria-hidden="true">→</span>}
                        {toVersion && (
                            <span className="update-progress__version update-progress__version--next">
                                {toVersion}
                            </span>
                        )}
                    </p>
                )}

                <div className="update-progress__bar">
                    <progress
                        max={100}
                        value={progress}
                        aria-label={t("jobs.progress")}
                    />
                    <span aria-hidden="true">{progress}&nbsp;%</span>
                </div>

                <ol className="update-progress__steps">
                    {PHASES.map((phase, index) => {
                        const done = index < current;
                        const active = index === current && !failed;
                        return (
                            <li
                                key={phase.id}
                                className={done
                                    ? "update-progress__step update-progress__step--done"
                                    : active
                                    ? "update-progress__step update-progress__step--active"
                                    : "update-progress__step"}
                            >
                                {done
                                    ? <Check size={14} aria-hidden="true" />
                                    : active
                                    ? <Loader2 size={14} aria-hidden="true" className="spin" />
                                    : <span className="update-progress__dot" aria-hidden="true" />}
                                {t(`server_detail.update_progress.steps.${phase.id}`)}
                            </li>
                        );
                    })}
                </ol>

                {failed && (
                    <p className="update-progress__failed" role="alert">
                        <TriangleAlert size={15} aria-hidden="true" />
                        {job?.error_message ?? t("server_detail.update_progress.failed")}
                    </p>
                )}

                {/* Sans elle, une phase longue — un téléchargement SteamCMD, par
                    exemple — laisse une barre immobile et rien pour distinguer
                    « ça travaille » de « c'est bloqué ». */}
                {latestLine && !failed && (
                    // `SafeAnsi` plutôt que le texte brut : la ligne arrive avec ses
                    // séquences d'échappement, qui s'affichaient telles quelles
                    // (« [32mServeur prêt [0m »).
                    <p className="update-progress__line" aria-live="polite">
                        <SafeAnsi>{latestLine}</SafeAnsi>
                    </p>
                )}

                {/* La croix suffit à fermer : un second bouton « Fermer » en pied
                    donnait deux commandes de même nom dans une seule fenêtre. */}
                <div className="update-progress__actions">
                    <Button variant="secondary" size="sm" onClick={onOpenTerminal}>
                        {t("server_detail.update_progress.open_terminal")}
                    </Button>
                </div>
            </div>
        </div>
    );
}
