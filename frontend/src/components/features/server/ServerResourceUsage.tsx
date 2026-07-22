import { Cpu, HardDrive, MemoryStick } from "lucide-react";
import type { CurrentServerMetric } from "@/schemas/operations";
import { useLanguage } from "@/contexts/LanguageContext";
import { formatBytes } from "@/utils/formatters";

interface ServerResourceUsageProps {
    metric?: CurrentServerMetric;
    running: boolean;
    compact?: boolean;
}

export default function ServerResourceUsage({ metric, running, compact = false }: ServerResourceUsageProps) {
    const { t } = useLanguage();
    return (
        <div className={`server-resource-usage ${compact ? "server-resource-usage--compact" : ""}`} aria-label={t("metrics.instance_resources")}>
            <span title={`CPU: ${running && metric ? `${metric.cpu_usage.toFixed(1)} %` : "—"}`}><Cpu aria-hidden="true" /><small>CPU</small><strong>{running && metric ? `${metric.cpu_usage.toFixed(1)}%` : "—"}</strong></span>
            <span title={`${t("metrics.ram")}: ${running && metric ? formatBytes(metric.memory_bytes) : "—"}`}><MemoryStick aria-hidden="true" /><small>{t("metrics.ram")}</small><strong>{running && metric ? formatBytes(metric.memory_bytes) : "—"}</strong></span>
            <span title={`${t("metrics.disk")}: ${metric ? formatBytes(metric.disk_bytes) : "—"}`}><HardDrive aria-hidden="true" /><small>{t("metrics.disk")}</small><strong>{metric ? formatBytes(metric.disk_bytes) : "—"}</strong></span>
        </div>
    );
}
