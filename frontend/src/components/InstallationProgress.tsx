import React, { useState, useEffect } from 'react';
import { Terminal, Download, Folder, Check } from 'lucide-react';

interface InstallationProgressProps {
    logs: string[];
    onClose: () => void;
    isInstalling: boolean;
}

const InstallationProgress: React.FC<InstallationProgressProps> = ({ logs, onClose, isInstalling }) => {
    // Determine current step based on logs
    const steps = [
        { id: 'init', label: 'Initialisation', icon: <Terminal size={18} /> },
        { id: 'download', label: 'Téléchargement', icon: <Download size={18} /> },
        { id: 'extract', label: 'Installation', icon: <Folder size={18} /> },
        { id: 'finish', label: 'Finalisation', icon: <Check size={18} /> },
    ];

    const [currentStep, setCurrentStep] = useState(0);

    useEffect(() => {
        const lastLog = logs[logs.length - 1] || '';
        // Simple state machine based on log messages from backend
        if (lastLog.includes('Initialization de l\'installation') || lastLog.includes('Starting Hytale Server Installation')) setCurrentStep(0);
        else if (lastLog.includes('Téléchargement')) setCurrentStep(1);
        else if (lastLog.includes('Extraction') || lastLog.includes('Décompression')) setCurrentStep(2);
        else if (lastLog.includes('Installation terminée') || lastLog.includes('Installation finished')) {
            setCurrentStep(3);
        }
    }, [logs]);

    // If not installing and no logs, don't show (sanity check, though parent should handle)
    if (!isInstalling && logs.length === 0) return null;

    return (
        <div className="installation-overlay">
            <div className="installation-overlay__card">
                <div className="installation-header">
                    <h2 className="installation-header__title">
                        Installation en cours...
                    </h2>
                    <p className="installation-header__subtitle">Veuillez ne pas fermer cette fenêtre</p>
                </div>

                <div className="installation-steps">
                    {steps.map((step, index) => {
                        const isComplete = currentStep > index;
                        const isCurrent = currentStep === index;

                        let modifierClass = '';
                        if (isCurrent) modifierClass = 'installation-step--current';
                        else if (isComplete) modifierClass = 'installation-step--complete';

                        return (
                            <div key={step.id} className={`installation-step ${modifierClass}`}>
                                <div className="installation-step__icon">
                                    {isComplete ? <Check size={14} /> : step.icon}
                                </div>
                                <div className="installation-step__content">
                                    <div className="installation-step__label">{step.label}</div>
                                    {isCurrent && (
                                        <div className="installation-step__loader">
                                            <div className="installation-step__loader-bar"></div>
                                        </div>
                                    )}
                                </div>
                            </div>
                        );
                    })}
                </div>

                {currentStep === 3 && (
                    <div className="installation-actions">
                        <button
                            onClick={onClose}
                            className="btn-finish"
                        >
                            Terminer
                        </button>
                    </div>
                )}
            </div>
        </div>
    );
};

export default InstallationProgress;
