import { useState, useEffect } from "react";
import { X, Save, Clock, Zap, Play, Power, RotateCw, HardDrive, AlertCircle } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Schedule } from "./ServerSchedule";
import { Select, Checkbox, Input } from "@/components/ui";

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
        cron_expression: "0 0 * * *",
    });

    // Helper for Cron simplified UI
    const [cronDays, setCronDays] = useState<number[]>([]); // 0-6
    const [cronHour, setCronHour] = useState("00:00");

    useEffect(() => {
        if (schedule) {
            setFormData(schedule);
            // Parse existing cron if possible for simplified UI
            if (schedule.type === "cron" && schedule.cron_expression) {
                const parts = schedule.cron_expression.split(" ");
                if (parts.length >= 5) {
                    const dow = parts[4];
                    if (dow === "*") setCronDays([0,1,2,3,4,5,6]);
                    else setCronDays(dow.split(",").map(Number));
                    setCronHour(`${parts[1].padStart(2, "0")}:${parts[0].padStart(2, "0")}`);
                }
            }
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
                cron_expression: "0 0 * * *",
            });
            setCronDays([0,1,2,3,4,5,6]);
            setCronHour("00:00");
        }
    }, [schedule, isOpen]);

    // Update cron_expression when simplified UI changes
    useEffect(() => {
        if (formData.type === "cron") {
            const [hour, minute] = cronHour.split(":");
            const dow = cronDays.length === 7 ? "*" : cronDays.length === 0 ? "*" : cronDays.join(",");
            const newCron = `${parseInt(minute)} ${parseInt(hour)} * * ${dow}`;
            if (newCron !== formData.cron_expression) {
                setFormData(prev => ({ ...prev, cron_expression: newCron }));
            }
        }
    }, [cronDays, cronHour, formData.type]);

    if (!isOpen) return null;

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        await onSave(formData);
        onClose();
    };

    const typeOptions = [
        { value: "basic", label: t("server_detail.schedule.types.basic"), icon: <Zap size={14} /> },
        { value: "cron", label: t("server_detail.schedule.types.cron"), icon: <Clock size={14} /> },
        { value: "chain", label: t("server_detail.schedule.types.chain"), icon: <RotateCw size={14} /> },
    ];

    const actionOptions = [
        { value: "start", label: t("server_detail.schedule.actions.start"), icon: <Play size={14} /> },
        { value: "restart", label: t("server_detail.schedule.actions.restart"), icon: <RotateCw size={14} /> },
        { value: "stop", label: t("server_detail.schedule.actions.stop"), icon: <Power size={14} /> },
        { value: "backup", label: t("server_detail.schedule.actions.backup"), icon: <HardDrive size={14} /> },
        { value: "command", label: t("server_detail.schedule.actions.command"), icon: <AlertCircle size={14} /> },
    ];

    const unitOptions = [
        { value: "minutes", label: t("server_detail.schedule.units.minutes") },
        { value: "hours", label: t("server_detail.schedule.units.hours") },
        { value: "days", label: t("server_detail.schedule.units.days") },
        { value: "weeks", label: t("server_detail.schedule.units.weeks") },
    ];

    const weekdays = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];
    // Maps Hytale/Cron (0=Sun) to our UI (0=Mon)
    const mapToCron = (idx: number) => (idx + 1) % 7;
    const mapFromCron = (val: number) => (val + 6) % 7;

    const toggleDay = (idx: number) => {
        const cronVal = mapToCron(idx);
        if (cronDays.includes(cronVal)) {
            setCronDays(cronDays.filter(d => d !== cronVal));
        } else {
            setCronDays([...cronDays, cronVal].sort());
        }
    };

    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="modal modal--large" onClick={e => e.stopPropagation()} style={{ minHeight: "550px", display: "flex", flexDirection: "column" }}>
                <div className="modal__header">
                    <h3>{schedule ? t("server_detail.schedule.edit_task") : t("server_detail.schedule.create_task")}</h3>
                    <button className="modal-close" onClick={onClose} style={{ background: "none", border: "none", color: "var(--text-secondary)", cursor: "pointer" }}><X size={20} /></button>
                </div>
                
                <form onSubmit={handleSubmit} style={{ flex: 1, display: "flex", flexDirection: "column" }}>
                    <div className="modal__body" style={{ flex: 1, overflow: "visible" }}>
                        <div className="form-grid">
                            <div className="form-group full-width">
                                <label className="form-label">{t("server_detail.schedule.name")}</label>
                                <Input
                                    type="text"
                                    value={formData.name}
                                    onChange={e => setFormData({ ...formData, name: e.target.value })}
                                    placeholder={t("server_detail.schedule.name_placeholder")}
                                    required
                                />
                            </div>

                            <div className="form-group">
                                <label className="form-label">{t("server_detail.schedule.type")}</label>
                                <Select
                                    options={typeOptions}
                                    value={formData.type || "basic"}
                                    onChange={val => setFormData({ ...formData, type: val as any })}
                                />
                            </div>

                            <div className="form-group">
                                <label className="form-label">{t("server_detail.schedule.action")}</label>
                                <Select
                                    options={actionOptions}
                                    value={formData.action || "restart"}
                                    onChange={val => setFormData({ ...formData, action: val as any })}
                                />
                            </div>

                            {formData.type === "basic" && (
                                <>
                                    <div className="form-group">
                                        <label className="form-label">{t("server_detail.schedule.interval")}</label>
                                        <div className="input-group">
                                            <span className="text-muted self-center">Chaque</span>
                                            <Input
                                                type="number"
                                                value={formData.interval}
                                                onChange={e => setFormData({ ...formData, interval: parseInt(e.target.value) })}
                                                min="1"
                                                style={{ width: "70px" }}
                                            />
                                            <Select
                                                className="flex-1"
                                                options={unitOptions}
                                                value={formData.unit || "days"}
                                                onChange={val => setFormData({ ...formData, unit: val as any })}
                                            />
                                        </div>
                                    </div>

                                    <div className="form-group">
                                        <label className="form-label">{t("server_detail.schedule.time")}</label>
                                        <Input
                                            type="time"
                                            value={formData.time}
                                            onChange={e => setFormData({ ...formData, time: e.target.value })}
                                            icon={<Clock size={16} />}
                                        />
                                    </div>
                                </>
                            )}

                            {formData.type === "cron" && (
                                <div className="full-width">
                                    <div className="cron-simple-ui card bg-darker p-4 mb-4">
                                        <label className="form-label mb-3">Exécuter ces jours :</label>
                                        <div className="weekday-selector mb-4">
                                            {weekdays.map((day, idx) => (
                                                <button
                                                    key={day}
                                                    type="button"
                                                    className={`weekday-btn ${cronDays.includes(mapToCron(idx)) ? "active" : ""}`}
                                                    onClick={() => toggleDay(idx)}
                                                >
                                                    {day}
                                                </button>
                                            ))}
                                        </div>
                                        <div className="form-group">
                                            <label className="form-label">À cette heure :</label>
                                            <Input
                                                type="time"
                                                value={cronHour}
                                                onChange={e => setCronHour(e.target.value)}
                                                icon={<Clock size={16} />}
                                            />
                                        </div>
                                    </div>

                                    <div className="form-group">
                                        <label className="form-label">Expression Cron (Avancé)</label>
                                        <Input
                                            type="text"
                                            value={formData.cron_expression || ""}
                                            onChange={e => setFormData({ ...formData, cron_expression: e.target.value })}
                                            placeholder="* * * * *"
                                        />
                                        <p className="form-help">Format: minute heure jour mois jour-semaine</p>
                                    </div>
                                </div>
                            )}

                            <div className="full-width flex-row mt-4">
                                <Checkbox
                                    checked={!!formData.enabled}
                                    onChange={val => setFormData({ ...formData, enabled: val })}
                                    label={t("server_detail.schedule.enabled")}
                                />

                                <Checkbox
                                    checked={!!formData.delete_after}
                                    onChange={val => setFormData({ ...formData, delete_after: val })}
                                    label={t("server_detail.schedule.delete_after")}
                                />
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
                .flex-row { display: flex; gap: 32px; align-items: center; }
                .flex-1 { flex: 1; }
                .self-center { align-self: center; }
                
                .form-group { display: flex; flex-direction: column; gap: 8px; }
                .form-label { font-size: 0.8rem; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; }
                .form-help { font-size: 0.75rem; color: var(--text-muted); margin-top: 4px; }
                
                .input-group { display: flex; gap: 12px; align-items: flex-start; }
                
                .bg-darker { background: rgba(0,0,0,0.2); border-radius: 8px; border: 1px solid var(--border-card); }
                
                .weekday-selector { display: flex; gap: 8px; flex-wrap: wrap; }
                .weekday-btn {
                    padding: 8px 12px;
                    border-radius: 6px;
                    border: 1px solid var(--border-card);
                    background: var(--bg-secondary);
                    color: var(--text-primary);
                    cursor: pointer;
                    font-size: 0.85rem;
                    transition: all 0.2s;
                }
                .weekday-btn:hover { border-color: var(--primary-color); }
                .weekday-btn.active {
                    background: var(--primary-color);
                    border-color: var(--primary-color);
                    color: white;
                }
                
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
                .flex-row { display: flex; gap: 32px; align-items: center; }
                .flex-1 { flex: 1; }
                .self-center { align-self: center; }
                
                .form-group { display: flex; flex-direction: column; gap: 8px; }
                .form-label { font-size: 0.8rem; font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; }
                .form-help { font-size: 0.75rem; color: var(--text-muted); margin-top: 4px; }
                
                .input-group { display: flex; gap: 12px; align-items: flex-start; }
                
                .bg-darker { background: rgba(0,0,0,0.2); border-radius: 8px; border: 1px solid var(--border-card); }
                
                .weekday-selector { display: flex; gap: 8px; flex-wrap: wrap; }
                .weekday-btn {
                    padding: 8px 12px;
                    border-radius: 6px;
                    border: 1px solid var(--border-card);
                    background: var(--bg-secondary);
                    color: var(--text-primary);
                    cursor: pointer;
                    font-size: 0.85rem;
                    transition: all 0.2s;
                }
                .weekday-btn:hover { border-color: var(--primary-color); }
                .weekday-btn.active {
                    background: var(--primary-color);
                    border-color: var(--primary-color);
                    color: white;
                }
                
                .mt-4 { margin-top: 16px; }
            `}</style>
        </div>
    );
}
