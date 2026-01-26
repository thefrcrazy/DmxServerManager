import React, { useState, useEffect } from 'react';
import { Terminal, Download, Folder, Check, AlertTriangle, ExternalLink } from 'lucide-react';

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
    const [authUrl, setAuthUrl] = useState<string | null>(null);
    const [authCode, setAuthCode] = useState<string | null>(null);

    useEffect(() => {
        const lastLog = logs[logs.length - 1] || '';

        // Simple state machine based on log messages
        if (lastLog.includes('Initialization de l\'installation') || lastLog.includes('Starting Hytale Server Installation')) setCurrentStep(0);
        else if (lastLog.includes('Téléchargement')) setCurrentStep(1);
        else if (lastLog.includes('Extraction') || lastLog.includes('Décompression')) setCurrentStep(2);
        else if (lastLog.includes('Installation terminée') || lastLog.includes('Installation finished')) setCurrentStep(3);

        // Check for Auth URL in ALL logs (since it might scroll past)
        const fullLog = logs.join('\n');

        // Pattern: https://oauth.accounts.hytale.com/...
        // The log usually lines are:
        // "Please visit the following URL to authenticate:"
        // "https://oauth.accounts.hytale.com/oauth2/device/verify?user_code=XXXXXX"
        const linkMatch = fullLog.match(/(https:\/\/oauth\.accounts\.hytale\.com\/[^\s]+)/);
        if (linkMatch) {
            setAuthUrl(linkMatch[1]);

            // Try to extract code from URL if present (user_code=...)
            const codeMatch = linkMatch[1].match(/user_code=([^&]+)/);
            if (codeMatch) {
                setAuthCode(codeMatch[1]);
            } else {
                // Fallback: Check for "Authorization code: XXXXXX"
                const manualCodeMatch = fullLog.match(/Authorization code:\s*([^\s]+)/);
                if (manualCodeMatch) setAuthCode(manualCodeMatch[1]);
            }
        }
    }, [logs]);

    // If not installing and no logs, don't show (sanity check)
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

                {/* Auth Action Required Bubble */}
                {authUrl && currentStep < 3 && (
                    <div className="installation-auth">
                        <div className="installation-auth__title">
                            <AlertTriangle size={18} /> Action Requise
                        </div>
                        <div className="installation-auth__content">
                            <p className="text-sm text-yellow-100/80 mb-1">Hytale nécessite une authentification :</p>
                            <a href={authUrl} target="_blank" rel="noopener noreferrer" className="installation-auth__link">
                                {authUrl} <ExternalLink size={12} style={{ display: 'inline', verticalAlign: 'middle' }} />
                            </a>
                            {authCode && (
                                <div>
                                    <span className="text-xs text-muted block mt-2">Code de vérification :</span>
                                    <span className="installation-auth__code">{authCode}</span>
                                </div>
                            )}
                        </div>
                    </div>
                )}

                <div className="installation-actions">
                    {currentStep === 3 ? (
                        <button onClick={onClose} className="btn-finish">
                            Terminer
                        </button>
                    ) : (
                        <button onClick={onClose} className="btn-cancel">
                            Annuler / Fermer
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
};

export default InstallationProgress;
