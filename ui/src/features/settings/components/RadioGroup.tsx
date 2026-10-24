import { useId } from "react";

interface RadioOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

interface RadioGroupProps<T extends string> {
  legend: string;
  value: T;
  options: RadioOption<T>[];
  onChange: (value: T) => void;
}

/** Native radio buttons in a fieldset (arrow keys move between options). */
export function RadioGroup<T extends string>({
  legend,
  value,
  options,
  onChange,
}: RadioGroupProps<T>) {
  const name = useId();
  return (
    <fieldset className="settings-radios">
      <legend>{legend}</legend>
      {options.map((option) => (
        <label key={option.value} className="settings-radios__option">
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={option.value === value}
            disabled={option.disabled ?? false}
            onChange={() => {
              onChange(option.value);
            }}
          />
          {option.label}
        </label>
      ))}
    </fieldset>
  );
}
