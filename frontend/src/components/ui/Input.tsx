import React from "react";

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
    icon?: React.ReactNode;
    error?: string;
}

const Input: React.FC<InputProps> = ({ icon, error, className = "", ...props }) => {
    return (
        <div className={`input-wrapper ${error ? "input-wrapper--error" : ""}`}>
            {icon && <span className="input-wrapper__icon">{icon}</span>}
            <input
                className={`input ${icon ? "input--with-icon" : ""} ${className}`}
                {...props}
            />
            {error && <span className="input-wrapper__error">{error}</span>}
        </div>
    );
};

export default Input;
