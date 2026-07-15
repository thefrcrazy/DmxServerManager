import eslint from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
    { ignores: ["dist", "node_modules", "playwright-report", "test-results"] },
    eslint.configs.recommended,
    ...tseslint.configs.recommended,
    {
        files: ["scripts/**/*.ts"],
        languageOptions: {
            globals: { ...globals.node, Bun: "readonly" },
        },
    },
    {
        files: ["**/*.{ts,tsx}"],
        languageOptions: {
            ecmaVersion: "latest",
            globals: globals.browser,
        },
        plugins: {
            "react-hooks": reactHooks,
            "react-refresh": reactRefresh,
        },
        rules: {
            "react-hooks/rules-of-hooks": "error",
            "react-hooks/exhaustive-deps": "error",
            "react-refresh/only-export-components": "off",
            "@typescript-eslint/no-explicit-any": "off",
            "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_", caughtErrors: "none" }],
            "@typescript-eslint/no-empty-object-type": "off",
            "@typescript-eslint/ban-ts-comment": "off",
            "no-control-regex": "off",
        },
    },
);
