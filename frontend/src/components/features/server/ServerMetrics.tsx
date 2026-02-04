import { useEffect, useState, useRef, useCallback } from "react";
import { useLanguage } from "@/contexts/LanguageContext";
import api from "@/services";

interface MetricsDataPoint {
    timestamp: Date;
    cpu: number;
    memory: number;
    players: number;
}

interface ServerMetricsProps {
    serverId: string;
    cpuUsage: number;
    ramUsage: number;
    diskUsage: number | null;
    maxHeapBytes?: number;
    serverStatus: string;
    currentPlayers?: number;
    maxPlayers?: number;
}

type Period = "1h" | "6h" | "1d" | "7d";

const PERIOD_CONFIG: Record<Period, { label: string; maxPoints: number; intervalMs: number }> = {
    "1h": { label: "1 Heure", maxPoints: 120, intervalMs: 30000 },      // 1 point/30s
    "6h": { label: "6 Heures", maxPoints: 720, intervalMs: 30000 },     // 1 point/30s
    "1d": { label: "1 Jour", maxPoints: 2880, intervalMs: 30000 },      // 1 point/30s
    "7d": { label: "7 Jours", maxPoints: 20160, intervalMs: 30000 },    // 1 point/30s
};

export default function ServerMetrics({
    serverId,
    cpuUsage,
    ramUsage,
    maxHeapBytes = 0,
    serverStatus,
    currentPlayers = 0,
    maxPlayers = 100,
}: ServerMetricsProps) {
    const { t } = useLanguage();
    const [history, setHistory] = useState<MetricsDataPoint[]>([]);
    const [period, setPeriod] = useState<Period>("1d");
    const [_isLoading, setIsLoading] = useState(true);
    const lastUpdateRef = useRef<number>(0);
    const canvasRef = useRef<HTMLCanvasElement>(null);

    // Load historical data from API
    const loadHistory = useCallback(async () => {
        setIsLoading(true);
        try {
            const response = await api.getServerMetrics(serverId, period);
            const apiData = response.data?.data || [];

            // Convert API data to our format
            const historicalData: MetricsDataPoint[] = apiData.map((point) => ({
                timestamp: new Date(point.recorded_at),
                cpu: point.cpu_usage,
                memory: maxHeapBytes > 0 ? (point.memory_bytes / maxHeapBytes) * 100 : 0,
                players: point.player_count,
            }));

            setHistory(historicalData);
        } catch (error) {
            console.error("Failed to load metrics history:", error);
            setHistory([]);
        } finally {
            setIsLoading(false);
        }
    }, [serverId, period, maxHeapBytes]);

    // Load history on mount and period change
    useEffect(() => {
        loadHistory();
    }, [loadHistory]);

    // Add real-time data points from live metrics
    useEffect(() => {
        if (serverStatus !== "running") return;

        const now = Date.now();
        if (now - lastUpdateRef.current >= 5000) { // Update display every 5 seconds
            lastUpdateRef.current = now;

            const ramPercent = maxHeapBytes > 0 ? (ramUsage / maxHeapBytes) * 100 : 0;

            setHistory(prev => {
                const config = PERIOD_CONFIG[period];
                const newHistory = [...prev, {
                    timestamp: new Date(),
                    cpu: cpuUsage,
                    memory: ramPercent,
                    players: currentPlayers,
                }];

                if (newHistory.length > config.maxPoints) {
                    return newHistory.slice(-config.maxPoints);
                }
                return newHistory;
            });
        }
    }, [cpuUsage, ramUsage, currentPlayers, maxHeapBytes, period, serverStatus]);

    // Draw the chart
    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        // Get CSS variables from the document
        const styles = getComputedStyle(document.documentElement);
        const bgColor = styles.getPropertyValue("--color-bg-primary").trim() || "#0d0d0d";
        const borderColor = styles.getPropertyValue("--color-border").trim() || "#2d2d2d";
        const textMuted = styles.getPropertyValue("--color-text-muted").trim() || "#6b7280";

        const dpr = window.devicePixelRatio || 1;
        const rect = canvas.getBoundingClientRect();

        canvas.width = rect.width * dpr;
        canvas.height = rect.height * dpr;
        ctx.scale(dpr, dpr);

        const width = rect.width;
        const height = rect.height;
        const padding = { top: 20, right: 20, bottom: 60, left: 50 };
        const chartWidth = width - padding.left - padding.right;
        const chartHeight = height - padding.top - padding.bottom;

        // Clear canvas
        ctx.fillStyle = bgColor;
        ctx.fillRect(0, 0, width, height);

        // Draw grid
        ctx.strokeStyle = borderColor;
        ctx.lineWidth = 1;

        // Horizontal grid lines
        const ySteps = 5;
        for (let i = 0; i <= ySteps; i++) {
            const y = padding.top + (i / ySteps) * chartHeight;
            ctx.beginPath();
            ctx.moveTo(padding.left, y);
            ctx.lineTo(width - padding.right, y);
            ctx.stroke();

            // Y-axis labels
            const value = ((ySteps - i) / ySteps * 100).toFixed(0);
            ctx.fillStyle = textMuted;
            ctx.font = "11px Inter, sans-serif";
            ctx.textAlign = "right";
            ctx.fillText(value, padding.left - 8, y + 4);
        }

        if (history.length < 2) {
            // No data message
            ctx.fillStyle = textMuted;
            ctx.font = "14px Inter, sans-serif";
            ctx.textAlign = "center";
            ctx.fillText(t("server_detail.metrics.no_data"), width / 2, height / 2);
            return;
        }

        // Draw X-axis labels (timestamps)
        const xLabels = 8;
        ctx.fillStyle = textMuted;
        ctx.font = "10px Inter, sans-serif";
        ctx.textAlign = "center";

        for (let i = 0; i < xLabels; i++) {
            const index = Math.floor((i / (xLabels - 1)) * (history.length - 1));
            const point = history[index];
            if (point) {
                const x = padding.left + (index / (history.length - 1)) * chartWidth;
                const time = point.timestamp;
                const label = `${time.getHours().toString().padStart(2, "0")}:${time.getMinutes().toString().padStart(2, "0")}`;

                ctx.save();
                ctx.translate(x, height - padding.bottom + 15);
                ctx.rotate(-Math.PI / 4);
                ctx.fillText(label, 0, 0);
                ctx.restore();
            }
        }

        // Draw data lines
        const drawLine = (data: number[], color: string, maxValue: number = 100) => {
            if (data.length < 2) return;

            ctx.beginPath();
            ctx.strokeStyle = color;
            ctx.lineWidth = 2;
            ctx.lineJoin = "round";

            data.forEach((value, i) => {
                const x = padding.left + (i / (data.length - 1)) * chartWidth;
                const y = padding.top + (1 - Math.min(value, maxValue) / maxValue) * chartHeight;

                if (i === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
            });
            ctx.stroke();

            // Draw points
            ctx.fillStyle = color;
            data.forEach((value, i) => {
                const x = padding.left + (i / (data.length - 1)) * chartWidth;
                const y = padding.top + (1 - Math.min(value, maxValue) / maxValue) * chartHeight;
                ctx.beginPath();
                ctx.arc(x, y, 3, 0, Math.PI * 2);
                ctx.fill();
            });
        };

        // Players line (purple, scaled to maxPlayers)
        const playersData = history.map(h => (h.players / maxPlayers) * 100);
        drawLine(playersData, "#a855f7");

        // Memory line (cyan/blue)
        drawLine(history.map(h => h.memory), "#22d3ee");

        // CPU line (orange)
        drawLine(history.map(h => h.cpu), "#f59e0b");

    }, [history, t, maxPlayers]);

    const isRunning = serverStatus === "running";

    return (
        <div className="server-metrics-chart">
            <div className="metrics-header">
                <div className="period-selector">
                    <label>{t("server_detail.metrics.period")}</label>
                    <select
                        value={period}
                        onChange={(e) => setPeriod(e.target.value as Period)}
                    >
                        {Object.entries(PERIOD_CONFIG).map(([key, config]) => (
                            <option key={key} value={key}>{config.label}</option>
                        ))}
                    </select>
                </div>

                <div className="legend">
                    <div className="legend-item">
                        <span className="legend-color" style={{ backgroundColor: "#a855f7" }} />
                        <span>Players</span>
                    </div>
                    <div className="legend-item">
                        <span className="legend-color" style={{ backgroundColor: "#22d3ee" }} />
                        <span>MEM</span>
                    </div>
                    <div className="legend-item">
                        <span className="legend-color" style={{ backgroundColor: "#f59e0b" }} />
                        <span>CPU</span>
                    </div>
                </div>
            </div>

            <div className="chart-container">
                <canvas ref={canvasRef} />

                {!isRunning && (
                    <div className="chart-overlay">
                        <span>{t("server_detail.metrics.server_offline")}</span>
                    </div>
                )}
            </div>

            <div className="current-stats">
                <div className="stat">
                    <span className="stat-label">CPU</span>
                    <span className="stat-value" style={{ color: "#f59e0b" }}>{cpuUsage.toFixed(1)}%</span>
                </div>
                <div className="stat">
                    <span className="stat-label">MEM</span>
                    <span className="stat-value" style={{ color: "#22d3ee" }}>
                        {maxHeapBytes > 0 ? ((ramUsage / maxHeapBytes) * 100).toFixed(1) : 0}%
                    </span>
                </div>
                <div className="stat">
                    <span className="stat-label">Players</span>
                    <span className="stat-value" style={{ color: "#a855f7" }}>{currentPlayers} / {maxPlayers}</span>
                </div>
            </div>
        </div>
    );
}
