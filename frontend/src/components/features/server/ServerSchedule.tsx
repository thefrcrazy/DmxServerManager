import { useState } from "react";
import { Clock, Plus, Trash2, Edit2, Play, Power, RotateCw, HardDrive, AlertCircle, Calendar } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import ScheduleModal from "./ScheduleModal";
import { Tooltip, Table } from "@/components/ui";
import cronstrue from "cronstrue/i18n";

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
    onRun: (id: string) => Promise<void>;
}

export default function ServerSchedule({
    schedules,
    isLoading,
    onSave,
    onDelete,
    onToggle,
    onRun,
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
            case "start": return <Play size={16} className="color-success" />;
            case "stop": return <Power size={16} className="color-danger" />;
            case "restart": return <RotateCw size={16} className="color-warning" />;
            case "backup": return <HardDrive size={16} className="color-info" />;
            case "command": return <AlertCircle size={16} />;
            default: return <Clock size={16} />;
        }
    };

    const formatScheduleTime = (schedule: Schedule) => {
        if (schedule.type === "cron" && schedule.cron_expression) {
            try {
                let desc = cronstrue.toString(schedule.cron_expression, { locale: "fr" });
                if (schedule.delete_after) {
                    desc = desc.replace("Tous les jours ", `${t("server_detail.schedule.at")} `);
                    desc = desc.replace("Chaque jour ", `${t("server_detail.schedule.at")} `);
                }
                return desc;
            } catch (e) {
                return schedule.cron_expression;
            }
        }
        if (schedule.type === "basic") {
            if (schedule.delete_after) {
                return `${t("server_detail.schedule.at")} ${schedule.time}`;
            }
            const unitLabel = t(`server_detail.schedule.units.${schedule.unit}`);
            if (schedule.unit === "days" && schedule.interval === 1) {
                return `${t("server_detail.schedule.every")} ${t("server_detail.schedule.units.days").toLowerCase()} ${t("server_detail.schedule.at").toLowerCase()} ${schedule.time}`;
            }
            return `${t("server_detail.schedule.every")} ${schedule.interval} ${unitLabel} ${t("server_detail.schedule.at").toLowerCase()} ${schedule.time}`;
        }
        return t("server_detail.schedule.types.chain");
    };

    return (
        <div className="schedules-wrapper">
            <div className="section-header">
                <div className="header-info">
                    <h3 className="section-title">
                        <Clock size={20} />
                        {t("server_detail.schedule.title")}
                    </h3>
                    <p className="section-subtitle">
                        {t("server_detail.schedule.subtitle")}
                    </p>
                </div>
                <button onClick={handleCreate} className="btn btn--primary btn--sm">
                    <Plus size={14} />
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
                        <Clock size={32} />
                        <p>{t("server_detail.schedule.no_tasks")}</p>
                        <button onClick={handleCreate} className="btn btn--secondary btn--sm mt-4">
                            {t("server_detail.schedule.create_first")}
                        </button>
                    </div>
                ) : (
                    <Table>
                        <thead>
                            <tr>
                                <th style={{ width: "40px" }}></th>
                                <th>{t("server_detail.schedule.name")}</th>
                                <th>{t("server_detail.schedule.action")}</th>
                                <th>{t("server_detail.schedule.time")}</th>
                                <th className="text-right">{t("common.actions")}</th>
                            </tr>
                        </thead>
                        <tbody>
                            {schedules.map((schedule) => (
                                <tr key={schedule.id} className={!schedule.enabled ? "table__row--disabled" : ""}>
                                    <td>
                                        <div className="status-toggle">
                                            <Tooltip content={schedule.enabled ? t("common.disable") : t("common.enable")} position="top">
                                                <input
                                                    type="checkbox"
                                                    checked={schedule.enabled}
                                                    onChange={(e) => onToggle(schedule.id, e.target.checked)}
                                                    className="switch-input"
                                                    id={`switch-${schedule.id}`}
                                                />
                                                <label htmlFor={`switch-${schedule.id}`} className="switch-label"></label>
                                            </Tooltip>
                                        </div>
                                    </td>
                                    <td>
                                        <div className="flex items-center gap-2">
                                            <span className="font-semibold">{schedule.name}</span>
                                            {schedule.delete_after && (
                                                <span className="task-badge task-badge--once">
                                                    {t("server_detail.schedule.execution_once")}
                                                </span>
                                            )}
                                        </div>
                                    </td>
                                    <td>
                                        <div className="flex items-center gap-2">
                                            {getActionIcon(schedule.action)}
                                            <span className="text-secondary">{t(`server_detail.schedule.actions.${schedule.action}`)}</span>
                                        </div>
                                    </td>
                                    <td>
                                        <div className="flex items-center gap-2 text-muted text-sm">
                                            <Calendar size={14} />
                                            {formatScheduleTime(schedule)}
                                        </div>
                                    </td>
                                    <td>
                                        <div className="table__actions">
                                            <Tooltip content={t("server_detail.schedule.run_now")} position="top">
                                                <button onClick={() => onRun(schedule.id)} className="btn btn--icon btn--ghost btn--sm btn--success">
                                                    <Play size={16} />
                                                </button>
                                            </Tooltip>
                                            <Tooltip content={t("common.edit")} position="top">
                                                <button onClick={() => handleEdit(schedule)} className="btn btn--icon btn--ghost btn--sm">
                                                    <Edit2 size={16} />
                                                </button>
                                            </Tooltip>
                                            <Tooltip content={t("common.delete")} position="top">
                                                <button onClick={() => onDelete(schedule.id)} className="btn btn--icon btn--ghost btn--sm btn--danger">
                                                    <Trash2 size={16} />
                                                </button>
                                            </Tooltip>
                                        </div>
                                    </td>
                                </tr>
                            ))}
                        </tbody>
                    </Table>
                )}
            </div>

            <ScheduleModal
                isOpen={isModalOpen}
                onClose={() => setIsModalOpen(false)}
                onSave={onSave}
                schedule={editingSchedule}
            />

            <style>{`
                .schedules-wrapper { display: flex; flex-direction: column; gap: var(--spacing-6); }
                .section-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: var(--spacing-4); }
                .section-title { display: flex; align-items: center; gap: var(--spacing-3); font-size: var(--font-size-lg); font-weight: 600; color: var(--color-text-primary); margin: 0; }
                .section-subtitle { font-size: var(--font-size-sm); color: var(--color-text-muted); margin: var(--spacing-1) 0 0 0; }
                
                .status-toggle { display: flex; align-items: center; }
                .switch-input { display: none; }
                .switch-label {
                    width: 32px; height: 16px;
                    background: var(--color-border);
                    border-radius: var(--radius-full);
                    position: relative;
                    cursor: pointer;
                    transition: var(--transition-fast);
                }
                .switch-input:checked + .switch-label { background: var(--color-accent); }
                .switch-label::after {
                    content: "";
                    position: absolute;
                    width: 12px; height: 12px;
                    background: white;
                    border-radius: 50%;
                    top: 2px; left: 2px;
                    transition: var(--transition-fast);
                }
                .switch-input:checked + .switch-label::after { transform: translateX(16px); }
                
                .color-success { color: var(--color-success); }
                .color-danger { color: var(--color-danger); }
                .color-warning { color: var(--color-warning); }
                .color-info { color: var(--color-info); }
                
                .task-badge {
                    display: inline-flex;
                    align-items: center;
                    padding: 0 6px;
                    height: 18px;
                    border-radius: var(--radius-sm);
                    font-size: 10px;
                    font-weight: 700;
                    text-transform: uppercase;
                    letter-spacing: 0.02em;
                }
                .task-badge--once {
                    background: rgba(245, 158, 11, 0.15);
                    color: var(--color-warning);
                    border: 1px solid rgba(245, 158, 11, 0.2);
                }
                
                .empty-state {
                    display: flex; flex-direction: column; align-items: center; justify-content: center;
                    padding: var(--spacing-8); color: var(--color-text-muted);
                    background: var(--color-bg-tertiary); border: 1px dashed var(--color-border); border-radius: var(--radius-md);
                }
                .mt-4 { margin-top: var(--spacing-4); }
                
                .tooltip-wrapper { display: inline-flex; }
                .text-right { text-align: right; }
                .flex { display: flex; }
                .items-center { align-items: center; }
                .gap-2 { gap: 0.5rem; }
                .font-semibold { font-weight: 600; }
                .text-secondary { color: var(--color-text-secondary); }
            `}</style>
        </div>
    );
}