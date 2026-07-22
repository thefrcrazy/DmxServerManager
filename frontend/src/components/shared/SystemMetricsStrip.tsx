import { Cpu, HardDrive, MemoryStick, Network } from "lucide-react";
import type { SystemMetricsSnapshot } from "@/schemas/operations";
import { useLanguage } from "@/contexts/LanguageContext";
import { formatBytes } from "@/utils/formatters";

interface SystemMetricsStripProps {
    metrics: SystemMetricsSnapshot | null;
    connected: boolean;
}

function percentage(used: number, total: number): number {
    return total > 0 ? Math.min(100, Math.max(0, used / total * 100)) : 0;
}

function ResourceMeter({ value }: { value: number }) {
    return <span className="system-metric__meter" aria-hidden="true"><span style={{ width: `${Math.min(100, Math.max(0, value))}%` }} /></span>;
}

export default function SystemMetricsStrip({ metrics, connected }: SystemMetricsStripProps) {
    const { t } = useLanguage();
    const cpu = metrics?.cpu_usage ?? 0;
    const memory = metrics ? percentage(metrics.memory_used_bytes, metrics.memory_total_bytes) : 0;
    const disk = metrics ? percentage(metrics.disk_used_bytes, metrics.disk_total_bytes) : 0;

    return (
        <section className="system-metrics" aria-label={t("metrics.system_resources") }>
            <header className="system-metrics__header">
                <div><strong>{t("metrics.system_resources")}</strong><span>{t("metrics.host_scope")}</span></div>
                <span className={`system-metrics__live ${connected ? "system-metrics__live--connected" : ""}`}>
                    <span aria-hidden="true" />{connected ? t("metrics.live") : t("metrics.reconnecting")}
                </span>
            </header>
            <div className="system-metrics__grid">
                <article className="system-metric"><Cpu size={17} aria-hidden="true" /><div><span>CPU</span><strong>{metrics ? `${cpu.toFixed(1)} %` : "—"}</strong><ResourceMeter value={cpu} /></div></article>
                <article className="system-metric"><MemoryStick size={17} aria-hidden="true" /><div><span>{t("metrics.ram")}</span><strong>{metrics ? `${formatBytes(metrics.memory_used_bytes)} / ${formatBytes(metrics.memory_total_bytes)}` : "—"}</strong><ResourceMeter value={memory} /></div></article>
                <article className="system-metric"><HardDrive size={17} aria-hidden="true" /><div><span>{t("metrics.disk")}</span><strong>{metrics ? `${formatBytes(metrics.disk_used_bytes)} / ${formatBytes(metrics.disk_total_bytes)}` : "—"}</strong><ResourceMeter value={disk} /></div></article>
                <article className="system-metric system-metric--network"><Network size={17} aria-hidden="true" /><div><span>{t("metrics.network")}</span><strong>{metrics ? `↓ ${formatBytes(metrics.network_receive_bytes_per_second)}/s · ↑ ${formatBytes(metrics.network_transmit_bytes_per_second)}/s` : "—"}</strong><small>{t("metrics.network_host_hint")}</small></div></article>
            </div>
        </section>
    );
}
