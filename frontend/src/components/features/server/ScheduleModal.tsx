import { useState, useEffect } from "react";
import { X, Save } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Schedule } from "./ServerSchedule";

interface ScheduleModalProps {
    isOpen: boolean;
    onClose: () => void;
    onSave: (schedule: Partial<Schedule>) => Promise<void>;
    schedule?: Schedule;
}

export default function ScheduleModal({ isOpen, onClose, onSave, schedule }: ScheduleModalProps) {
    const { t } = useLanguage();
    
    const [formData, setFormData] = useState<Partial<Schedule>>({
        name: "",
        type: "basic",
        action: "restart",
        interval: 1,
        unit: "days",
        time: "00:00",
        enabled: true,
        delete_after: false,
    });

    useEffect(() => {
        if (schedule) {
            setFormData(schedule);
        } else {
            setFormData({
                name: "",
                type: "basic",
                action: "restart",
                interval: 1,
                unit: "days",
                time: "00:00",
                enabled: true,
                delete_after: false,
            });
        }
    }, [schedule, isOpen]);

    if (!isOpen) return null;

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        await onSave(formData);
        onClose();
    };

    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="modal modal--large" onClick={e => e.stopPropagation()}>
                <div className="modal__header">
                    <h3>{schedule ? t("server_detail.schedule.edit_task") : t("server_detail.schedule.create_task")}</h3>
                    <button className="modal-close" onClick={onClose}><X size={20} /></button>
                </div>
                
                <form onSubmit={handleSubmit}>
                    <div className="modal__body">
                        <div className="form-grid">
                            <div className="form-group full-width">
                                <label className="form-label">{t("server_detail.schedule.name")}</label>
                                <input
                                    type="text"
                                    className="input"
                                    value={formData.name}
                                    onChange={e => setFormData({ ...formData, name: e.target.value })}
                                    placeholder={t("server_detail.schedule.name_placeholder")}
                                    required
                                />
                            </div>

                            <div className="form-group">
                                <label className="form-label">{t("server_detail.schedule.type")}</label>
                                <select
                                    className="select"
                                    value={formData.type}
                                    onChange={e => setFormData({ ...formData, type: e.target.value as any })}
                                >
                                    <option value="basic">{t("server_detail.schedule.types.basic")}</option>
                                    <option value="cron">{t("server_detail.schedule.types.cron")}</option>
                                    <option value="chain">{t("server_detail.schedule.types.chain")}</option>
                                </select>
                            </div>

                            <div className="form-group">
                                <label className="form-label">{t("server_detail.schedule.action")}</label>
                                <select
                                    className="select"
                                    value={formData.action}
                                    onChange={e => setFormData({ ...formData, action: e.target.value as any })}
                                >
                                    <option value="start">{t("server_detail.schedule.actions.start")}</option>
                                    <option value="restart">{t("server_detail.schedule.actions.restart")}</option>
                                    <option value="stop">{t("server_detail.schedule.actions.stop")}</option>
                                    <option value="backup">{t("server_detail.schedule.actions.backup")}</option>
                                    <option value="command">{t("server_detail.schedule.actions.command")}</option>
                                </select>
                            </div>

                            {formData.type === "basic" && (
                                <>
                                    <div className="form-group">
                                        <label className="form-label">{t("server_detail.schedule.interval")}</label>
                                        <div className="input-group">
                                            <input
                                                type="number"
                                                className="input"
                                                value={formData.interval}
                                                onChange={e => setFormData({ ...formData, interval: parseInt(e.target.value) })}
                                                min="1"
                                            />
                                            <select
                                                className="select"
                                                value={formData.unit}
                                                onChange={e => setFormData({ ...formData, unit: e.target.value as any })}
                                            >
                                                <option value="minutes">{t("server_detail.schedule.units.minutes")}</option>
                                                <option value="hours">{t("server_detail.schedule.units.hours")}</option>
                                                <option value="days">{t("server_detail.schedule.units.days")}</option>
                                                <option value="weeks">{t("server_detail.schedule.units.weeks")}</option>
                                            </select>
                                        </div>
                                    </div>

                                    <div className="form-group">
                                        <label className="form-label">{t("server_detail.schedule.time")}</label>
                                        <input
                                            type="time"
                                            className="input"
                                            value={formData.time}
                                            onChange={e => setFormData({ ...formData, time: e.target.value })}
                                        />
                                    </div>
                                </>
                            )}

                            {formData.type === "cron" && (
                                <div className="form-group full-width">
                                    <label className="form-label">Cron Expression</label>
                                    <input
                                        type="text"
                                        className="input"
                                        value={formData.cron_expression || ""}
                                        onChange={e => setFormData({ ...formData, cron_expression: e.target.value })}
                                        placeholder="* * * * *"
                                    />
                                    <p className="form-help">Format: minute hour dom month dow</p>
                                </div>
                            )}

                            <div className="form-group full-width flex-row">
                                <label className="checkbox-container">
                                    <input
                                        type="checkbox"
                                        checked={formData.enabled}
                                        onChange={e => setFormData({ ...formData, enabled: e.target.checked })}
                                    />
                                    <span className="checkbox-label">{t("server_detail.schedule.enabled")}</span>
                                </label>

                                <label className="checkbox-container">
                                    <input
                                        type="checkbox"
                                        checked={formData.delete_after}
                                        onChange={e => setFormData({ ...formData, delete_after: e.target.checked })}
                                    />
                                    <span className="checkbox-label">{t("server_detail.schedule.delete_after")}</span>
                                </label>
                            </div>
                        </div>
                    </div>
                    
                    <div className="modal__footer">
                        <button type="button" className="btn btn--secondary" onClick={onClose}>
                            {t("common.cancel")}
                        </button>
                        <button type="submit" className="btn btn--primary">
                            <Save size={18} />
                            {t("common.save")}
                        </button>
                    </div>
                </form>
            </div>

            <style>{`
                .form-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; }
                .full-width { grid-column: span 2; }
                .flex-row { display: flex; gap: 24px; align-items: center; }
                
                .form-group { display: flex; flex-direction: column; gap: 8px; }
                .form-label { font-size: 0.9rem; font-weight: 500; color: var(--text-secondary); }
                .form-help { font-size: 0.75rem; color: var(--text-muted); margin-top: 4px; }
                
                .input-group { display: flex; gap: 8px; }
                .input-group .input { flex: 1; }
                .input-group .select { flex: 1.5; }
                
                .checkbox-container { display: flex; align-items: center; gap: 10px; cursor: pointer; }
                .checkbox-label { font-size: 0.95rem; }
            `}</style>
        </div>
    );
}
