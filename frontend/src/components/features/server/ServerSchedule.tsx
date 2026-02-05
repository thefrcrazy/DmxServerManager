import { useState } from "react";
import { Clock, Plus, Trash2, Edit2, Play, Power, RotateCw, HardDrive, AlertCircle } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import ScheduleModal from "./ScheduleModal";

export interface Schedule {
    id: string;
    server_id: string;
    name: string;
    type: "basic" | "cron" | "chain";
    action: "start" | "restart" | "stop" | "backup" | "command";
    interval: number;
    unit: "minutes" | "hours" | "days" | "weeks";
    time: string;
    cron_expression?: string;
    enabled: boolean;
    delete_after: boolean;
    created_at: string;
}

interface ServerScheduleProps {
    schedules: Schedule[];
    isLoading: boolean;
    onSave: (schedule: Partial<Schedule>) => Promise<void>;
    onDelete: (id: string) => Promise<void>;
    onToggle: (id: string, enabled: boolean) => Promise<void>;
}

export default function ServerSchedule({
    schedules,
    isLoading,
    onSave,
    onDelete,
    onToggle,
}: ServerScheduleProps) {
    const { t } = useLanguage();
    const [isModalOpen, setIsModalOpen] = useState(false);
    const [editingSchedule, setEditingSchedule] = useState<Schedule | undefined>(undefined);

    const handleCreate = () => {
        setEditingSchedule(undefined);
        setIsModalOpen(true);
    };

    const handleEdit = (schedule: Schedule) => {
        setEditingSchedule(schedule);
        setIsModalOpen(true);
    };

    const getActionIcon = (action: string) => {
        switch (action) {
            case "start": return <Play size={16} className="text-success" />;
            case "stop": return <Power size={16} className="text-danger" />;
            case "restart": return <RotateCw size={16} className="text-warning" />;
            case "backup": return <HardDrive size={16} className="text-primary" />;
            case "command": return <AlertCircle size={16} />;
            default: return <Clock size={16} />;
        }
    };

    return (
        <div className="schedules-wrapper">
            <div className="section-header">
                <div className="header-info">
                    <h3 className="section-title">
                        <Clock size={24} />
                        {t("server_detail.schedule.title")}
                    </h3>
                    <p className="section-subtitle">
                        {t("server_detail.schedule.subtitle")}
                    </p>
                </div>
                <button onClick={handleCreate} className="btn btn--primary">
                    <Plus size={18} />
                    {t("server_detail.schedule.create_task")}
                </button>
            </div>

            <div className="list-container">
                {isLoading ? (
                    <div className="loading-container">
                        <div className="spinner"></div>
                    </div>
                ) : schedules.length === 0 ? (
                    <div className="empty-state">
                        <div className="icon-circle">
                            <Clock size={32} />
                        </div>
                        <h4>{t("server_detail.schedule.no_tasks")}</h4>
                        <p>{t("server_detail.schedule.empty_desc")}</p>
                        <button onClick={handleCreate} className="btn btn--secondary mt-4">
                            {t("server_detail.schedule.create_first")}
                        </button>
                    </div>
                ) : (
                    <div className="schedule-list">
                        {schedules.map((schedule) => (
                            <div key={schedule.id} className={`schedule-item ${!schedule.enabled ? "schedule-item--disabled" : ""}`}>
                                <div className="schedule-info">
                                    <div className="schedule-status">
                                        <input
                                            type="checkbox"
                                            checked={schedule.enabled}
                                            onChange={(e) => onToggle(schedule.id, e.target.checked)}
                                            className="switch"
                                        />
                                    </div>
                                    <div className="schedule-details">
                                        <div className="schedule-name">
                                            {schedule.name}
                                            {schedule.delete_after && (
                                                <span className="badge badge--sm badge--warning ml-2">
                                                    Once
                                                </span>
                                            )}
                                        </div>
                                        <div className="schedule-meta">
                                            <span className="meta-item">
                                                {getActionIcon(schedule.action)}
                                                {t(`server_detail.schedule.actions.${schedule.action}`)}
                                            </span>
                                            <span className="meta-separator">•</span>
                                            <span className="meta-item">
                                                <Clock size={12} />
                                                {schedule.type === "cron" 
                                                    ? schedule.cron_expression 
                                                    : `${schedule.interval} ${t(`server_detail.schedule.units.${schedule.unit}`)} @ ${schedule.time}`}
                                            </span>
                                        </div>
                                    </div>
                                </div>
                                <div className="schedule-actions">
                                    <button
                                        onClick={() => handleEdit(schedule)}
                                        className="btn btn--icon btn--ghost"
                                    >
                                        <Edit2 size={18} />
                                    </button>
                                    <button
                                        onClick={() => onDelete(schedule.id)}
                                        className="btn btn--icon btn--ghost btn--danger"
                                    >
                                        <Trash2 size={18} />
                                    </button>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>

            <ScheduleModal
                isOpen={isModalOpen}
                onClose={() => setIsModalOpen(false)}
                onSave={onSave}
                schedule={editingSchedule}
            />

            <style>{`
                .schedule-list { display: flex; flex-direction: column; gap: 12px; }
                .schedule-item {
                    display: flex; align-items: center; justify-content: space-between;
                    padding: 16px;
                    background: var(--bg-secondary);
                    border: 1px solid var(--border-card);
                    border-radius: 8px;
                    transition: all 0.2s;
                }
                .schedule-item:hover { border-color: var(--primary-color); }
                .schedule-item--disabled { opacity: 0.6; }
                
                .schedule-info { display: flex; align-items: center; gap: 16px; }
                .schedule-details { display: flex; flex-direction: column; gap: 4px; }
                .schedule-name { font-weight: 600; font-size: 1rem; color: var(--text-primary); }
                
                .schedule-meta { display: flex; align-items: center; gap: 8px; font-size: 0.85rem; color: var(--text-muted); }
                .meta-item { display: flex; align-items: center; gap: 6px; }
                .meta-separator { color: var(--text-muted); opacity: 0.5; }
                
                .schedule-actions { display: flex; gap: 8px; }
                
                .ml-2 { margin-left: 8px; }
                
                /* Switch Style */
                .switch {
                    appearance: none;
                    width: 36px; height: 20px;
                    background: var(--bg-tertiary);
                    border-radius: 10px;
                    position: relative;
                    cursor: pointer;
                    transition: background 0.2s;
                }
                .switch:checked { background: var(--primary-color); }
                .switch::before {
                    content: "";
                    position: absolute;
                    width: 16px; height: 16px;
                    background: white;
                    border-radius: 50%;
                    top: 2px; left: 2px;
                    transition: transform 0.2s;
                }
                .switch:checked::before { transform: translateX(16px); }
            `}</style>
        </div>
    );
}
