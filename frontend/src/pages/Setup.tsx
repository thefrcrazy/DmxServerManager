import { useState, useEffect } from "react";
import { Check, HardDrive, Palette, User, ArrowRight, ArrowLeft, FolderSearch } from "lucide-react";
import DirectoryPicker from "@/components/shared/DirectoryPicker";
import { PRESET_COLORS, applyAccentColor } from "@/constants/theme";
import "../styles/pages/_login.scss";

const STEPS = [
    { id: 1, key: "step1", icon: User },
    { id: 2, key: "step2", icon: HardDrive },
    { id: 3, key: "step3", icon: Palette },
    { id: 4, key: "step4", icon: Check },
];

import { useLanguage } from "../contexts/LanguageContext";

export default function Setup() {
    const { t } = useLanguage();
    const [currentStep, setCurrentStep] = useState(1);
    const [formData, setFormData] = useState({
        username: "admin",
        password: "",
        servers_dir: "./data/servers",
        backups_dir: "./data/backups",
        theme_color: "#3A82F6"
    });

    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState("");
    const [showServersDirPicker, setShowServersDirPicker] = useState(false);
    const [showBackupsDirPicker, setShowBackupsDirPicker] = useState(false);

    useEffect(() => {
        applyAccentColor(formData.theme_color);
    }, [formData.theme_color]);

    const nextStep = () => {
        if (currentStep === 1 && formData.password.length < 8) {
            setError(t("user_settings.password_min_length"));
            return;
        }
        setError("");
        setCurrentStep(prev => Math.min(prev + 1, 4));
    };

    const prevStep = () => {
        setError("");
        setCurrentStep(prev => Math.max(prev - 1, 1));
    };

    const handleSubmit = async () => {
        setIsLoading(true);
        setError("");

        try {
            const response = await fetch("/api/v1/setup", {
                method: "POST",
                headers: { "Content-Type": "application/json" },
                body: JSON.stringify(formData)
            });

            if (response.ok) {
                const data = await response.json();
                localStorage.setItem("token", data.token);
                window.location.href = "/";
            } else {
                const err = await response.json();
                setError(err.error ? t(err.error) : t("common.error"));
            }
        } catch (e) {
            setError(t("common.error"));
        } finally {
            setIsLoading(false);
        }
    };

    return (
        <div className="setup-page">
            <div className="setup-wizard">
                {/* Logo centré et plus gros */}
                <div className="setup-wizard__header">
                    <img
                        src="/draveur-manager-logo.png"
                        alt="Draveur Manager"
                        className="setup-wizard__logo"
                    />
                    <h1 className="setup-wizard__title">
                        {t("setup.title")}
                    </h1>
                    <p className="setup-wizard__subtitle">
                        {t("setup.subtitle")}
                    </p>
                </div>

                {/* Stepper */}
                <div className="wizard-steps">
                    {STEPS.map((step, index) => {
                        const StepIcon = step.icon;
                        const isActive = currentStep === step.id;
                        const isCompleted = currentStep > step.id;

                        return (
                            <div key={step.id} className="step-item">
                                <div className="step-item__content">
                                    <div className={`step-item__circle ${isCompleted ? "step-item__circle--completed" : isActive ? "step-item__circle--active" : ""}`}>
                                        {isCompleted ? <Check size={18} /> : <StepIcon size={18} />}
                                    </div>
                                    <span className={`step-item__label ${isActive ? "step-item__label--active" : ""}`}>
                                        {t(`setup.${step.key}`)}
                                    </span>
                                </div>
                                {index < STEPS.length - 1 && (
                                    <div className={`step-item__line ${isCompleted ? "step-item__line--completed" : ""}`} />
                                )}
                            </div>
                        );
                    })}
                </div>

                {error && (
                    <div className="alert alert--danger mb-6">
                        {error}
                    </div>
                )}

                {/* Step Content */}
                <div className="step-content-scroll">
                    {currentStep === 1 && (
                        <div className="step-content">
                            <h3 className="step-content__title">
                                <User size={20} className="text-accent" />
                                {t("setup.create_admin")}
                            </h3>
                            <div className="form-group mb-4">
                                <label className="form-label">{t("setup.username")}</label>
                                <input
                                    type="text"
                                    className="form-input"
                                    value={formData.username}
                                    onChange={e => setFormData({ ...formData, username: e.target.value })}
                                    required
                                />
                            </div>
                            <div className="form-group">
                                <label className="form-label">{t("setup.password")}</label>
                                <input
                                    type="password"
                                    className="form-input"
                                    value={formData.password}
                                    onChange={e => setFormData({ ...formData, password: e.target.value })}
                                    placeholder={t("setup.password_placeholder")}
                                    required
                                />
                            </div>
                        </div>
                    )}

                    {currentStep === 2 && (
                        <div className="step-content">
                            <h3 className="step-content__title">
                                <HardDrive size={20} className="text-accent" />
                                {t("setup.step2")}
                            </h3>
                            <p className="step-content__description">
                                {t("setup.paths_desc")}
                            </p>
                            <div className="form-group mb-4">
                                <label className="form-label">{t("setup.servers_dir")}</label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        className="form-input flex-1"
                                        value={formData.servers_dir}
                                        onChange={e => setFormData({ ...formData, servers_dir: e.target.value })}
                                        required
                                    />
                                    <button
                                        type="button"
                                        className="btn btn--secondary"
                                        onClick={() => setShowServersDirPicker(true)}
                                        title="Parcourir"
                                    >
                                        <FolderSearch size={18} />
                                    </button>
                                </div>
                            </div>
                            <div className="form-group">
                                <label className="form-label">{t("setup.backups_dir")}</label>
                                <div className="flex gap-2">
                                    <input
                                        type="text"
                                        className="form-input flex-1"
                                        value={formData.backups_dir}
                                        onChange={e => setFormData({ ...formData, backups_dir: e.target.value })}
                                        required
                                    />
                                    <button
                                        type="button"
                                        className="btn btn--secondary"
                                        onClick={() => setShowBackupsDirPicker(true)}
                                        title="Parcourir"
                                    >
                                        <FolderSearch size={18} />
                                    </button>
                                </div>
                            </div>
                        </div>
                    )}

                    {currentStep === 3 && (
                        <div className="step-content">
                            <h3 className="step-content__title">
                                <Palette size={20} className="text-accent" />
                                {t("setup.theme_title")}
                            </h3>
                            <div className="color-picker">
                                {PRESET_COLORS.map((color) => (
                                    <button
                                        key={color}
                                        type="button"
                                        onClick={() => setFormData(prev => ({ ...prev, theme_color: color }))}
                                        className={`color-picker__btn ${formData.theme_color.toLowerCase() === color.toLowerCase() ? "color-picker__btn--active" : ""}`}
                                        style={{ background: color, color: color }}
                                    >
                                        {formData.theme_color.toLowerCase() === color.toLowerCase() && (
                                            <Check size={24} color="white" strokeWidth={3} />
                                        )}
                                    </button>
                                ))}
                            </div>
                        </div>
                    )}

                    {currentStep === 4 && (
                        <div className="step-content">
                            <h3 className="step-content__title">
                                <Check size={20} className="text-accent" />
                                {t("setup.step4")}
                            </h3>
                            <div className="setup-summary">
                                <div className="setup-summary__row">
                                    <span className="setup-summary__label">{t("setup.summary_user")}</span>
                                    <span className="setup-summary__value">{formData.username}</span>
                                </div>
                                <div className="setup-summary__divider" />
                                <div className="setup-summary__row">
                                    <span className="setup-summary__label">{t("setup.summary_servers")}</span>
                                    <code className="setup-summary__value setup-summary__value--code">{formData.servers_dir}</code>
                                </div>
                                <div className="setup-summary__row">
                                    <span className="setup-summary__label">{t("setup.summary_backups")}</span>
                                    <code className="setup-summary__value setup-summary__value--code">{formData.backups_dir}</code>
                                </div>
                                <div className="setup-summary__divider" />
                                <div className="setup-summary__row">
                                    <span className="setup-summary__label">{t("setup.summary_theme")}</span>
                                    <div className="flex items-center gap-2">
                                        <div
                                            className="summary-color-preview"
                                            style={{ background: formData.theme_color }}
                                        />
                                        <span className="text-primary font-mono">
                                            {formData.theme_color}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        </div>
                    )}
                </div>

                {/* Navigation Buttons */}
                <div className="wizard-navigation">
                    {currentStep > 1 ? (
                        <button
                            type="button"
                            className="btn btn--secondary flex-1"
                            onClick={prevStep}
                        >
                            <ArrowLeft size={18} className="mr-2" />
                            {t("setup.prev")}
                        </button>
                    ) : (
                        <div className="flex-1" />
                    )}

                    {currentStep < 4 ? (
                        <button
                            type="button"
                            className="btn btn--primary flex-1"
                            onClick={nextStep}
                        >
                            {t("setup.next")}
                            <ArrowRight size={18} className="ml-2" />
                        </button>
                    ) : (
                        <button
                            type="button"
                            className="btn btn--primary flex-1"
                            onClick={handleSubmit}
                            disabled={isLoading}
                        >
                            {isLoading ? (
                                <div className="spinner-sm" />
                            ) : (
                                <>
                                    {t("setup.finish")}
                                    <Check size={18} className="ml-2" />
                                </>
                            )}
                        </button>
                    )}
                </div>
            </div>

            {/* Directory Pickers */}
            <DirectoryPicker
                isOpen={showServersDirPicker}
                onClose={() => setShowServersDirPicker(false)}
                onSelect={(path) => setFormData(prev => ({ ...prev, servers_dir: path }))}
                initialPath="/"
                title={t("setup.servers_dir_title")}
            />
            <DirectoryPicker
                isOpen={showBackupsDirPicker}
                onClose={() => setShowBackupsDirPicker(false)}
                onSelect={(path) => setFormData(prev => ({ ...prev, backups_dir: path }))}
                initialPath="/"
                title={t("setup.backups_dir_title")}
            />
        </div>
    );
}
