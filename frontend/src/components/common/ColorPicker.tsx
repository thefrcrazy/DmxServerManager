import { Check } from 'lucide-react';
import { PRESET_COLORS } from '../../constants/theme';

interface ColorPickerProps {
    value: string;
    onChange: (color: string) => void;
    showCustomInput?: boolean;
}

export default function ColorPicker({ value, onChange, showCustomInput = true }: ColorPickerProps) {
    return (
        <div className="color-picker">
            {PRESET_COLORS.map((color) => (
                <button
                    key={color}
                    type="button"
                    onClick={() => onChange(color)}
                    className={`color-picker__swatch ${value.toLowerCase() === color.toLowerCase() ? 'color-picker__swatch--active' : ''}`}
                    style={{
                        background: color,
                        boxShadow: value.toLowerCase() === color.toLowerCase()
                            ? `0 0 15px ${color}66`
                            : 'none'
                    }}
                >
                    {value.toLowerCase() === color.toLowerCase() && (
                        <Check size={20} color="white" strokeWidth={3} />
                    )}
                </button>
            ))}

            {showCustomInput && (
                <div className="color-picker__custom">
                    <input
                        type="color"
                        value={value}
                        onChange={(e) => onChange(e.target.value)}
                        title="Couleur personnalisée"
                    />
                </div>
            )}
        </div>
    );
}
