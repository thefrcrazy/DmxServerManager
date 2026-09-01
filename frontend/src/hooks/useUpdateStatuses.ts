import { useCallback, useEffect, useMemo, useState } from "react";
import type { GameUpdateStatus } from "@/schemas/api";
import { apiService } from "@/services";

/**
 * Verdicts de mise à jour de toutes les instances visibles, indexés par identifiant.
 *
 * Un seul appel, servi depuis les verdicts déjà conservés côté serveur : aucune
 * vérification n'est déclenchée. La liste des serveurs et le tableau de bord
 * peuvent donc afficher l'état sans lancer un processus externe par instance.
 */
export function useUpdateStatuses(): {
    statuses: Record<string, GameUpdateStatus>;
    outdatedCount: number;
    refresh: () => Promise<void>;
} {
    const [statuses, setStatuses] = useState<Record<string, GameUpdateStatus>>({});

    const refresh = useCallback(async () => {
        const response = await apiService.servers.listUpdateStatus();
        if (!response.success) return;
        setStatuses(Object.fromEntries(response.data.items.map(({ instance_id, ...status }) => [
            instance_id,
            status,
        ])));
    }, []);

    useEffect(() => { void refresh(); }, [refresh]);

    const outdatedCount = useMemo(
        () => Object.values(statuses).filter((status) => status.state === "update_available").length,
        [statuses],
    );

    return { statuses, outdatedCount, refresh };
}
