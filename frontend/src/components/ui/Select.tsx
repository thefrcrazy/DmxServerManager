import React, { useState, useRef, useEffect } from "react";
import { ChevronDown, Check } from "lucide-react";

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
}

const Select: React.FC<SelectProps> = ({
    options,
    value,
    onChange,
    placeholder = "Sélectionner...",
    disabled = false,
    className = ""
}) => {
    const [isOpen, setIsOpen] = useState(false);
    const containerRef = useRef<HTMLDivElement>(null);

    const selectedOption = options.find(opt => opt.value === value);

    // Close on click outside
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        };

        document.addEventListener("mousedown", handleClickOutside);
        return () => document.removeEventListener("mousedown", handleClickOutside);
    }, []);

    const handleSelect = (optionValue: string) => {
        if (disabled) return;
        onChange(optionValue);
        setIsOpen(false);
    };

    return (
        <div
            className={`custom-select ${className}`}
            ref={containerRef}
        >
            <div
                className={`custom-select__trigger ${isOpen ? "custom-select__trigger--open" : ""} ${disabled ? "custom-select__trigger--disabled" : ""}`}
                onClick={() => !disabled && setIsOpen(!isOpen)}
            >
                <div className="custom-select__value">
                    {selectedOption ? (
                        <>
                            {selectedOption.icon && <span className="custom-select__icon">{selectedOption.icon}</span>}
                            {selectedOption.label}
                        </>
                    ) : (
                        <span className="text-muted">{placeholder}</span>
                    )}
                </div>
                <ChevronDown size={16} className="custom-select__arrow" />
            </div>

            {isOpen && !disabled && (
                <div className="custom-select__options">
                    {options.map((option) => (
                        <div
                            key={option.value}
                            className={`custom-select__option ${option.value === value ? "custom-select__option--selected" : ""
                                } ${option.disabled ? "custom-select__option--disabled" : ""}`}
                            onClick={() => !option.disabled && handleSelect(option.value)}
                        >
                            {option.icon && <span className="custom-select__icon">{option.icon}</span>}
                            <span>{option.label}</span>
                            {option.value === value && (
                                <Check size={16} className="custom-select__option__check" />
                            )}
                        </div>
                    ))}
                    {options.length === 0 && (
                        <div className="custom-select__option custom-select__option--disabled">
                            Aucune option
                        </div>
                    )}
                </div>
            )}
        </div>
    );
};

export default Select;
