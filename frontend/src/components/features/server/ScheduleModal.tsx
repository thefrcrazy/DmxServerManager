import { useState, useEffect } from "react";
import { X, Save, Clock, Zap, Play, Power, RotateCw, HardDrive, AlertCircle } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Schedule } from "./ServerSchedule";
import { Select, Checkbox, Input } from "@/components/ui";
import cronstrue from "cronstrue/i18n";

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

    const [cronDays, setCronDays] = useState<number[]>([]);
    const [cronHour, setCronHour] = useState("00:00");
    const [cronDesc, setCronDesc] = useState("");

    useEffect(() => {
        if (schedule) {
            setFormData(schedule);
            if (schedule.type === "cron" && schedule.cron_expression) {
                const parts = schedule.cron_expression.split(" ");
                if (parts.length >= 5) {
                    const dow = parts[4];
                    if (dow === "*") setCronDays([0,1,2,3,4,5,6]);
                    else setCronDays(dow.split(",").map(Number));
                    const min = parts[0].padStart(2, "0");
                    const hour = parts[1].padStart(2, "0");
                    setCronHour(`${hour}:${min}`);
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

    useEffect(() => {
        if (formData.type === "cron" && isOpen) {
             const [hour, minute] = cronHour.split(":");
             const dow = cronDays.length === 7 ? "*" : cronDays.length === 0 ? "*" : cronDays.join(",");
             if (hour && minute) {
                 const newCron = `${parseInt(minute)} ${parseInt(hour)} * * ${dow}`;
                 if (newCron !== formData.cron_expression) {
                     setFormData(prev => ({ ...prev, cron_expression: newCron }));
                 }
             }
        }
    }, [cronDays, cronHour, formData.type, isOpen]);

    useEffect(() => {
        if (formData.cron_expression) {
            try {
                setCronDesc(cronstrue.toString(formData.cron_expression, { locale: "fr" }));
            } catch (e) {
                setCronDesc("");
            }
        }
    }, [formData.cron_expression]);

    if (!isOpen) return null;

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        await onSave(formData);
        onClose();
    };

    const typeOptions = [
        { value: "basic", label: t("server_detail.schedule.types.basic"), icon: <Zap size={14} /> },
        { value: "cron", label: t("server_detail.schedule.types.cron"), icon: <Clock size={14} /> },
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

    const weekdays = [
        { id: 1, label: "Lun" }, { id: 2, label: "Mar" }, { id: 3, label: "Mer" },
        { id: 4, label: "Jeu" }, { id: 5, label: "Ven" }, { id: 6, label: "Sam" }, { id: 0, label: "Dim" }
    ];

    const toggleDay = (dayId: number) => {
        if (cronDays.includes(dayId)) setCronDays(cronDays.filter(d => d !== dayId));
        else setCronDays([...cronDays, dayId].sort());
    };

    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="modal modal-schedule" onClick={e => e.stopPropagation()}>
                <div className="modal__header">
                    <h3 className="modal__title">{schedule ? t("server_detail.schedule.edit_task") : t("server_detail.schedule.create_task")}</h3>
                    <button className="modal__close" onClick={onClose}><X size={18} /></button>
                </div>
                
                <form onSubmit={handleSubmit} className="modal__form">
                    <div className="modal__body">
                        <div className="form-section">
                            <label className="field-label">{t("server_detail.schedule.name").toUpperCase()}</label>
                            <Input
                                value={formData.name}
                                onChange={e => setFormData({ ...formData, name: e.target.value })}
                                placeholder={t("server_detail.schedule.name_placeholder")}
                                required
                            />
                        </div>

                        <div className="form-row">
                            <div className="form-section">
                                <label className="field-label">{t("server_detail.schedule.type").toUpperCase()}</label>
                                <Select options={typeOptions} value={formData.type || "basic"} onChange={val => setFormData({ ...formData, type: val as any })} />
                            </div>
                            <div className="form-section">
                                <label className="field-label">{t("server_detail.schedule.action").toUpperCase()}</label>
                                <Select options={actionOptions} value={formData.action || "restart"} onChange={val => setFormData({ ...formData, action: val as any })} />
                            </div>
                        </div>

                        {formData.type === "basic" ? (
                            <div className="form-row">
                                <div className="form-section">
                                    <label className="field-label">{t("server_detail.schedule.interval").toUpperCase()}</label>
                                    <div className="interval-group">
                                        <span>{t("server_detail.schedule.every")}</span>
                                        <Input type="number" value={formData.interval} onChange={e => setFormData({ ...formData, interval: parseInt(e.target.value) })} min="1" className="small-input" />
                                        <Select options={unitOptions} value={formData.unit || "days"} onChange={val => setFormData({ ...formData, unit: val as any })} />
                                    </div>
                                </div>
                                <div className="form-section">
                                    <label className="field-label">{t("server_detail.schedule.time").toUpperCase()}</label>
                                    <Input type="time" value={formData.time} onChange={e => setFormData({ ...formData, time: e.target.value })} icon={<Clock size={14} />} />
                                </div>
                            </div>
                        ) : (
                            <div className="cron-container">
                                <div className="cron-simple">
                                    <div className="cron-simple__header">
                                        <label className="field-label">{t("server_detail.schedule.simplified_config").toUpperCase()}</label>
                                        <button type="button" className="text-btn" onClick={() => setCronDays([0,1,2,3,4,5,6])}>{t("server_detail.schedule.select_all")}</button>
                                    </div>
                                    <div className="days-picker">
                                        <label className="sub-label">{t("server_detail.schedule.days_label")}</label>
                                        <div className="days-row">
                                            {weekdays.map(day => (
                                                <button key={day.id} type="button" className={`day-btn ${cronDays.includes(day.id) ? "active" : ""}`} onClick={() => toggleDay(day.id)}>{day.label}</button>
                                            ))}
                                        </div>
                                    </div>
                                    <div className="time-picker mt-3">
                                        <label className="sub-label">{t("server_detail.schedule.time_label")}</label>
                                        <Input type="time" value={cronHour} onChange={e => setCronHour(e.target.value)} icon={<Clock size={14} />} />
                                    </div>
                                </div>

                                <div className="cron-advanced mt-4">
                                    <label className="field-label">{t("server_detail.schedule.cron_advanced").toUpperCase()}</label>
                                    <Input value={formData.cron_expression || ""} onChange={e => setFormData({ ...formData, cron_expression: e.target.value })} placeholder="* * * * *" />
                                    {cronDesc && <div className="cron-hint"><Zap size={12} /> {cronDesc}</div>}
                                    <p className="cron-help">{t("server_detail.schedule.cron_hint")}</p>
                                </div>
                            </div>
                        )}

                        <div className="form-footer-toggles mt-4">
                            <Checkbox checked={!!formData.enabled} onChange={val => setFormData({ ...formData, enabled: val })} label={t("server_detail.schedule.enabled")} />
                            <Checkbox checked={!!formData.delete_after} onChange={val => setFormData({ ...formData, delete_after: val })} label={t("server_detail.schedule.delete_after")} />
                        </div>
                    </div>
                    
                    <div className="modal__footer">
                        <div className="footer-actions">
                            <button type="button" className="btn btn--secondary" onClick={onClose}>{t("common.cancel")}</button>
                            <button type="submit" className="btn btn--primary">
                                <Save size={16} />
                                {t("common.save")}
                            </button>
                        </div>
                    </div>
                </form>
            </div>

            <style>{`
                .modal-schedule { width: 520px; background: var(--color-bg-secondary); border: 1px solid var(--color-border); border-radius: var(--radius-md); box-shadow: var(--shadow-lg); }
                .modal__header { padding: var(--spacing-5) var(--spacing-6); border-bottom: 1px solid var(--color-border); display: flex; justify-content: space-between; align-items: center; }
                .modal__title { font-size: var(--font-size-lg); font-weight: 600; margin: 0; }
                .modal__close { background: none; border: none; color: var(--color-text-muted); cursor: pointer; padding: var(--spacing-1); }
                .modal__close:hover { color: var(--color-text-primary); }
                
                .modal__body { padding: var(--spacing-6); display: flex; flex-direction: column; gap: var(--spacing-5); }
                .form-section { display: flex; flex-direction: column; gap: var(--spacing-3); }
                .form-row { display: grid; grid-template-columns: 1fr 1fr; gap: var(--spacing-5); }
                
                .field-label { font-size: var(--font-size-xs); font-weight: 700; color: var(--color-text-muted); letter-spacing: 0.05em; }
                .sub-label { font-size: var(--font-size-sm); color: var(--color-text-primary); margin-bottom: var(--spacing-2); display: block; }
                
                .interval-group { display: flex; align-items: center; gap: var(--spacing-3); color: var(--color-text-primary); font-size: var(--font-size-sm); }
                .small-input { width: 60px !important; }
                
                .cron-container { background: var(--color-bg-tertiary); padding: var(--spacing-5); border-radius: var(--radius-md); border: 1px solid var(--color-border); }
                .cron-simple__header { display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--spacing-4); }
                .text-btn { background: none; border: none; color: var(--color-text-secondary); font-size: var(--font-size-sm); cursor: pointer; padding: 0; }
                .text-btn:hover { color: var(--color-accent); text-decoration: underline; }
                
                .days-row { display: flex; gap: var(--spacing-2); }
                .day-btn { flex: 1; height: 32px; background: none; border: 1px solid var(--color-border); color: var(--color-text-muted); border-radius: var(--radius-md); cursor: pointer; font-size: var(--font-size-xs); transition: var(--transition-fast); }
                .day-btn:hover { border-color: var(--color-border-hover); color: var(--color-text-primary); }
                .day-btn.active { background: var(--color-accent); border-color: var(--color-accent); color: var(--color-text-inverse); font-weight: 600; }
                
                .cron-hint { margin-top: var(--spacing-3); font-size: var(--font-size-sm); color: var(--color-text-primary); display: flex; align-items: center; gap: var(--spacing-2); }
                .cron-help { font-size: var(--font-size-xs); color: var(--color-text-muted); margin-top: var(--spacing-1); }
                
                .form-footer-toggles { display: flex; gap: var(--spacing-8); padding-top: var(--spacing-4); border-top: 1px solid var(--color-border); }
                
                .modal__footer { padding: var(--spacing-4) var(--spacing-6); background: var(--color-bg-tertiary); border-top: 1px solid var(--color-border); border-radius: 0 0 var(--radius-md) var(--radius-md); }
                .footer-actions { display: flex; justify-content: flex-end; gap: var(--spacing-3); }
                
                .mt-3 { margin-top: var(--spacing-3); }
                .mt-4 { margin-top: var(--spacing-4); }
            `}</style>
        </div>
    );
}