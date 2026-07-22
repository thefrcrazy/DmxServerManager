import { Activity, Clock3, HardDrive, LoaderCircle, MemoryStick, Users } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { MetricPeriod, MetricPeriodSchema, MetricPoint } from "@/schemas/operations";
import { apiService } from "@/services";
import { formatBytes } from "@/utils/formatters";

interface ServerMetricsProps {
    instanceId: string;
    isRunning: boolean;
    refreshSignal?: number;
}

interface SparklineProps {
    points: MetricPoint[];
    value: (point: MetricPoint) => number;
    label: string;
    format: (value: number) => string;
    minimumMaximum?: number;
}

function Sparkline({ points, value, label, format, minimumMaximum = 0 }: SparklineProps) {
    const values = points.map(value);
    const maximum = Math.max(minimumMaximum, ...values, 1);
    const polyline = values.map((sample, index) => {
        const x = values.length <= 1 ? 0 : (index / (values.length - 1)) * 600;
        const y = 150 - (sample / maximum) * 140;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
    }).join(" ");
    const latest = values.at(-1) ?? 0;
    return (
        <figure className="metric-chart">
            <figcaption><strong>{label}</strong><span>{format(latest)}</span></figcaption>
            <svg viewBox="0 0 600 160" role="img" aria-label={`${label}: ${format(latest)}`} preserveAspectRatio="none">
                <line x1="0" y1="150" x2="600" y2="150" />
                <polyline points={polyline} />
            </svg>
        </figure>
    );
}

function formatUptime(seconds: number, dayLabel: string): string {
    const days = Math.floor(seconds / 86_400);
    const hours = Math.floor((seconds % 86_400) / 3_600);
    const minutes = Math.floor((seconds % 3_600) / 60);
    return `${days > 0 ? `${days}${dayLabel} ` : ""}${hours}h ${minutes}m`;
}

export default function ServerMetrics({ instanceId, isRunning, refreshSignal = 0 }: ServerMetricsProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const [period, setPeriod] = useState<MetricPeriod>("1d");
    const [points, setPoints] = useState<MetricPoint[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const load = useCallback(async () => {
        const response = await apiService.metrics.history(instanceId, period);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setPoints(response.data.points);
        setError(null);
    }, [instanceId, period]);

    useEffect(() => {
        setLoading(true);
        void load();
    }, [load, refreshSignal]);

    const latest = points.at(-1);
    const recent = useMemo(() => points.slice(-50).reverse(), [points]);

    return (
        <section className="server-metrics card" aria-labelledby="metrics-heading">
            <div className="server-metrics__header">
                <div><h2 id="metrics-heading">{t("server_detail.metrics.title")}</h2><p>{t("server_detail.metrics.retention")}</p></div>
                <label className="metric-period">
                    <span>{t("server_detail.metrics.period")}</span>
                    <select className="select" value={period} onChange={(event) => {
                        const parsed = MetricPeriodSchema.safeParse(event.target.value);
                        if (parsed.success) setPeriod(parsed.data);
                        else toast.error(t("server_detail.metrics.invalid_period"));
                    }}>
                        <option value="1h">1 h</option><option value="6h">6 h</option><option value="1d">24 h</option><option value="7d">7 j</option>
                    </select>
                </label>
            </div>
            {!isRunning && <p className="operations-warning" role="status">{t("server_detail.metrics.server_offline")}</p>}
            {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
            {error && <div className="operations-error" role="alert">{error}<Button size="sm" variant="secondary" onClick={() => void load()}>{t("administration.retry")}</Button></div>}
            {!loading && !error && !latest && <div className="operations-empty"><Activity aria-hidden="true" /><p>{t("server_detail.metrics.no_data")}</p></div>}
            {!loading && !error && latest && <>
                <div className="metric-summary">
                    <div><Activity aria-hidden="true" /><span>{t("server_detail.metrics.cpu")}</span><strong>{latest.cpu_usage.toFixed(1)} %</strong></div>
                    <div><MemoryStick aria-hidden="true" /><span>{t("server_detail.metrics.ram")}</span><strong>{formatBytes(latest.memory_bytes)}</strong></div>
                    <div><HardDrive aria-hidden="true" /><span>{t("server_detail.metrics.disk")}</span><strong>{formatBytes(latest.disk_bytes)}</strong></div>
                    <div><Clock3 aria-hidden="true" /><span>{t("server_detail.metrics.uptime")}</span><strong>{formatUptime(latest.uptime_seconds, t("server_detail.metrics.day_short"))}</strong></div>
                    <div><Users aria-hidden="true" /><span>{t("server_detail.metrics.players")}</span><strong>{latest.player_count ?? 0}</strong></div>
                </div>
                <div className="metric-charts">
                    <Sparkline points={points} value={(point) => point.cpu_usage} label={t("server_detail.metrics.cpu_history")} format={(value) => `${value.toFixed(1)} %`} minimumMaximum={100} />
                    <Sparkline points={points} value={(point) => point.memory_bytes} label={t("server_detail.metrics.memory_history")} format={formatBytes} />
                </div>
                <details className="metric-samples">
                    <summary>{t("server_detail.metrics.samples")}</summary>
                    <div className="table-scroll"><table>
                        <thead><tr><th>{t("server_detail.metrics.recorded_at")}</th><th>{t("server_detail.metrics.cpu")}</th><th>{t("server_detail.metrics.ram")}</th><th>{t("server_detail.metrics.disk")}</th><th>{t("server_detail.metrics.players")}</th></tr></thead>
                        <tbody>{recent.map((point) => <tr key={point.id}><td><time dateTime={point.recorded_at}>{new Date(point.recorded_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time></td><td>{point.cpu_usage.toFixed(1)} %</td><td>{formatBytes(point.memory_bytes)}</td><td>{formatBytes(point.disk_bytes)}</td><td>{point.player_count ?? "—"}</td></tr>)}</tbody>
                    </table></div>
                </details>
            </>}
        </section>
    );
}
