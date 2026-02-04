import React from "react";

interface CheckboxProps {
    checked: boolean;
    onChange: (checked: boolean) => void;
    label: React.ReactNode;
    description?: string;
    disabled?: boolean;
    className?: string;
}

const Checkbox: React.FC<CheckboxProps> = ({
    checked,
    onChange,
    label,
    description,
    disabled = false,
    className = ""
}) => {
    return (
        <label
            className={`checkbox-component ${description ? "checkbox-component--with-description" : ""} ${disabled ? "checkbox-component--disabled" : ""} ${className}`}
        >
            <input
                type="checkbox"
                checked={checked}
                onChange={(e) => !disabled && onChange(e.target.checked)}
                disabled={disabled}
            />
            <div className="checkbox-component__content">
                <span className="checkbox-component__label">
                    {label}
                </span>
                {description && (
                    <span className="checkbox-component__description">
                        {description}
                    </span>
                )}
            </div>
        </label>
    );
};

export default Checkbox;
