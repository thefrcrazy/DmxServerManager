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
    const [downloadProgress, setDownloadProgress] = useState<{ percent: number; details: string } | null>(null);
    const logsContainerRef = React.useRef<HTMLDivElement>(null);

    // Auto-scroll logs
    useEffect(() => {
        if (logsContainerRef.current) {
            logsContainerRef.current.scrollTop = logsContainerRef.current.scrollHeight;
        }
    }, [logs]);

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

        // Check for progress bar in recent logs (look at last few lines)
        // Format: [==========] 27.0% (385.0 MB / 1.4 GB)
        // Regex: \[\=*\s*\]\s*([\d\.]+)%\s*\(([^)]+)\)
        let foundProgress = false;
        // Search from end to find most recent progress
        for (let i = logs.length - 1; i >= Math.max(0, logs.length - 10); i--) {
            const line = logs[i];
            const progressMatch = line.match(/\[=*[\s=]*\]\s*([\d\.]+)%\s*\(([^)]+)\)/);
            if (progressMatch) {
                setDownloadProgress({
                    percent: parseFloat(progressMatch[1]),
                    details: progressMatch[2]
                });
                foundProgress = true;
                break;
            }
        }

        // If we found progress, it implies Auth is passed.
        // If NO progress found yet, check/keep auth.
        if (foundProgress) {
            setAuthUrl(null);
            setAuthCode(null);
        } else if (linkMatch) {
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
        } else {
            // If no linkMatch and no progress, ensure auth states are cleared
            setAuthUrl(null);
            setAuthCode(null);
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
                {authUrl && currentStep < 3 && !downloadProgress && (
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

                {/* Real-time Progress Bar */}
                {downloadProgress && currentStep < 3 && (
                    <div className="installation-download">
                        <div className="installation-download__details">
                            <span>Téléchargement des fichiers...</span>
                            <span className="installation-download__percent">{downloadProgress.percent}%</span>
                        </div>
                        <div className="installation-download__bar-container">
                            <div
                                className="installation-download__bar-fill"
                                style={{ width: `${downloadProgress.percent}%` }}
                            ></div>
                        </div>
                        <div className="installation-download__details" style={{ justifyContent: 'flex-end', opacity: 0.7 }}>
                            {downloadProgress.details}
                        </div>
                    </div>
                )}

                {/* Fallback Log Status (Show if no progress bar valid or if Auth needed) */}
                {logs.length > 0 && currentStep < 3 && !downloadProgress && !authUrl && (
                    <div className="installation-status">
                        <span className="installation-status__prefix">&gt;</span>
                        {(() => {
                            // Get last non-empty log, ignoring simple newlines
                            const lastLines = logs.filter(l => l.trim().length > 0);
                            const lastLine = lastLines[lastLines.length - 1] || '...';
                            return lastLine.replace(/^\[ERR\]\s*/, '').substring(0, 80) + (lastLine.length > 80 ? '...' : '');
                        })()}
                    </div>
                )}

                {/* Detailed Logs Collapsible - Always shown to user request */}
                <details className="installation-details">
                    <summary className="installation-details__summary">
                        <Terminal size={12} className="mr-2" /> Voir en détails ({logs.length})
                    </summary>
                    <div className="installation-details__content" ref={logsContainerRef}>
                        {logs.length === 0 ? (
                            <div className="text-muted italic opacity-50">En attente de logs...</div>
                        ) : (
                            logs.map((log, i) => (
                                <div key={i} className="log-line">{log}</div>
                            ))
                        )}
                    </div>
                </details>
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
