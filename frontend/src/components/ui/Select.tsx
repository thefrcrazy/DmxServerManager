import React from "react";
import { ChevronDown } from "lucide-react";

export interface Option {
    label: string;
    value: string;
    icon?: React.ReactNode;
    disabled?: boolean;
}

interface SelectProps {
    options: Option[];
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    disabled?: boolean;
    className?: string;
    id?: string;
    name?: string;
    "aria-label"?: string;
}

const Select: React.FC<SelectProps> = ({
    options,
    value,
    onChange,
    placeholder = "Sélectionner...",
    disabled = false,
    className = "",
    id,
    name,
    "aria-label": ariaLabel,
}) => {
    return (
        <div className={`custom-select ${className}`}>
            <select
                id={id}
                name={name}
                className="custom-select__trigger"
                value={value}
                disabled={disabled}
                aria-label={ariaLabel ?? placeholder}
                onChange={(event) => onChange(event.target.value)}
            >
                {!options.some((option) => option.value === value) && <option value="">{placeholder}</option>}
                {options.map((option) => <option key={option.value} value={option.value} disabled={option.disabled}>{option.label}</option>)}
            </select>
            <ChevronDown size={16} className="custom-select__arrow" aria-hidden="true" />
        </div>
    );
};

export default Select;
