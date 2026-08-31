import React, { useId } from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
    icon?: React.ReactNode;
    error?: string;
}

const Input: React.FC<InputProps> = ({ icon, error, className = "", id, ...props }) => {
    // Le message d'erreur était rendu sans lien programmatique : un lecteur
    // d'écran ne signalait ni l'état invalide ni la raison du refus.
    const generatedId = useId();
    const inputId = id ?? generatedId;
    const errorId = `${inputId}-error`;

    return (
        <div className={`input-wrapper ${error ? "input-wrapper--error" : ""}`}>
            {icon && <span className="input-wrapper__icon">{icon}</span>}
            <input
                id={inputId}
                className={`input ${icon ? "input--with-icon" : ""} ${className}`}
                aria-invalid={error ? true : undefined}
                aria-describedby={error ? errorId : props["aria-describedby"]}
                {...props}
            />
            {error && <span id={errorId} className="input-wrapper__error">{error}</span>}
        </div>
    );
};

export default Input;
