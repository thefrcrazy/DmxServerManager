import React, { useRef } from "react";

interface RangeSliderProps {
    min: number;
    max: number;
    value: number;
    onChange: (value: number) => void;
    step?: number;
    className?: string;
    showValue?: boolean;
}

const RangeSlider: React.FC<RangeSliderProps> = ({
    min,
    max,
    value,
    onChange,
    step = 1,
    className = "",
    showValue = true
}) => {
    const inputRef = useRef<HTMLInputElement>(null);

    // Calculate progress percentage for the fill track
    const progress = ((value - min) / (max - min)) * 100;

    return (
        <div className={`mistral-range ${className}`}>
            <div className="mistral-range__input-wrapper">
                <div className="mistral-range__track-bg" />
                <div
                    className="mistral-range__track-fill"
                    style={{ width: `${progress}%` }}
                />
                <input
                    ref={inputRef}
                    type="range"
                    min={min}
                    max={max}
                    step={step}
                    value={value}
                    onChange={(e) => onChange(Number(e.target.value))}
                />
            </div>
            {showValue && (
                <span className="mistral-range__value">{value}</span>
            )}
        </div>
    );
};

export default RangeSlider;
