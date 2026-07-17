import { LoaderCircle, RefreshCw, ShieldCheck, UserRound, Users } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import type { PlayerSnapshot } from "@/schemas/operations";
import { formatDate } from "@/utils/formatters";
import ServerConfigFiles from "./ServerConfigFiles";

interface ServerPlayersProps {
    instanceId: string;
    snapshot: PlayerSnapshot | null;
    loading: boolean;
    error: string | null;
    canReadAccess: boolean;
    canWriteAccess: boolean;
    isRunning: boolean;
    refreshSignal?: number;
    onRefresh: () => void;
}

export default function ServerPlayers({
    instanceId,
    snapshot,
    loading,
    error,
    canReadAccess,
    canWriteAccess,
    isRunning,
    refreshSignal = 0,
    onRefresh,
}: ServerPlayersProps) {
    const { t, language } = useLanguage();
    const locale = language === "fr" ? "fr-FR" : "en-US";

    return (
        <div className="server-players">
            <section className="card server-players__presence" aria-labelledby="server-players-heading">
                <header className="server-players__header">
                    <div>
                        <h2 id="server-players-heading"><Users size={20} aria-hidden="true" />{t("server_detail.players.title")}</h2>
                        <p>{t("server_detail.players.description")}</p>
                    </div>
                    <Button type="button" size="sm" variant="ghost" icon={<RefreshCw size={15} />} onClick={onRefresh}>{t("common.refresh")}</Button>
                </header>

                {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
                {error && <div className="operations-error" role="alert">{error}</div>}
                {!loading && snapshot && (
                    <>
                        <div className="server-players__summary">
                            <div><Users aria-hidden="true" /><span>{t("server_detail.players.online")}</span><strong>{snapshot.online_count}</strong></div>
                            <div><UserRound aria-hidden="true" /><span>{t("server_detail.players.known")}</span><strong>{snapshot.players.length}</strong></div>
                            <div><ShieldCheck aria-hidden="true" /><span>{t("server_detail.players.detection")}</span><strong>{t(`server_detail.players.detection_modes.${snapshot.detection}`)}</strong></div>
                        </div>

                        {snapshot.players.length === 0 ? (
                            <div className="operations-empty"><Users aria-hidden="true" /><p>{t(snapshot.detection === "unavailable" ? "server_detail.players.detection_unavailable" : "server_detail.players.empty")}</p></div>
                        ) : (
                            <div className="table-scroll server-players__table">
                                <table>
                                    <thead><tr>
                                        <th>{t("server_detail.players.player")}</th>
                                        <th>{t("server_detail.players.status")}</th>
                                        <th>{t("server_detail.players.identifier")}</th>
                                        <th>{t("server_detail.players.last_seen")}</th>
                                    </tr></thead>
                                    <tbody>
                                        {snapshot.players.map((player) => (
                                            <tr key={player.player_key}>
                                                <td><strong>{player.display_name}</strong><small>{player.source.replaceAll("_", " ")}</small></td>
                                                <td><span className={`badge badge--${player.online ? "success" : "muted"}`}>{t(player.online ? "server_detail.players.connected" : "server_detail.players.offline")}</span></td>
                                                <td><code title={player.external_id ?? player.player_key}>{player.external_id ?? "—"}</code></td>
                                                <td><time dateTime={player.last_seen_at}>{formatDate(player.last_seen_at, locale)}</time></td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            </div>
                        )}
                    </>
                )}
            </section>

            {snapshot && (
                <section className="card server-player-access" aria-labelledby="server-player-access-heading">
                    <header>
                        <h2 id="server-player-access-heading"><ShieldCheck size={20} aria-hidden="true" />{t("server_detail.players.access_title")}</h2>
                        <p>{t(`server_detail.players.access_modes.${snapshot.access_mode}`)}</p>
                    </header>
                    {snapshot.access_mode === "native_files" && canReadAccess && (
                        <ServerConfigFiles
                            instanceId={instanceId}
                            category="access"
                            canWrite={canWriteAccess}
                            isRunning={isRunning}
                            refreshSignal={refreshSignal}
                        />
                    )}
                    {snapshot.access_mode === "native_files" && !canReadAccess && (
                        <p className="server-player-access__permission-note">{t("server_detail.players.access_read_required")}</p>
                    )}
                </section>
            )}
        </div>
    );
}
