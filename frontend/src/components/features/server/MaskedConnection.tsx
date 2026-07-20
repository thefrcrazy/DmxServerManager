import { Copy, Eye, EyeOff } from "lucide-react";
import { useState } from "react";
import { Button, Tooltip } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type { ConnectionInfo } from "@/schemas/api";

interface MaskedConnectionProps {
    connection?: ConnectionInfo;
    compact?: boolean;
}

export default function MaskedConnection({ connection, compact = false }: MaskedConnectionProps) {
    const { t } = useLanguage();
    const toast = useToast();
    const [revealed, setRevealed] = useState(false);
    const endpoint = connection?.endpoints.find((candidate) => candidate.primary) ?? connection?.endpoints[0];

    const copy = async () => {
        if (!endpoint?.address) return;
        try {
            await navigator.clipboard.writeText(endpoint.address);
            toast.success(t("server_detail.connection.copied"));
        } catch {
            toast.error(t("server_detail.connection.copy_failed"));
        }
    };

    if (!connection || !connection.configured || !endpoint?.address) {
        return <span className="masked-connection masked-connection--missing">{connection ? t("server_detail.connection.not_configured") : "—"}</span>;
    }

    return (
        <span className={`masked-connection ${compact ? "masked-connection--compact" : ""}`} onClick={(event) => event.stopPropagation()}>
            <code aria-live="polite">{revealed ? endpoint.address : `••••••:${endpoint.port}`}</code>
            <span className="masked-connection__actions">
                <Tooltip content={t(revealed ? "server_detail.connection.hide" : "server_detail.connection.reveal")} position="top">
                    <Button type="button" variant="ghost" size="icon" aria-label={t(revealed ? "server_detail.connection.hide" : "server_detail.connection.reveal")} onClick={() => setRevealed((value) => !value)}>{revealed ? <EyeOff size={14} /> : <Eye size={14} />}</Button>
                </Tooltip>
                <Tooltip content={t("server_detail.connection.copy")} position="top">
                    <Button type="button" variant="ghost" size="icon" aria-label={t("server_detail.connection.copy")} onClick={() => void copy()}><Copy size={14} /></Button>
                </Tooltip>
            </span>
        </span>
    );
}
